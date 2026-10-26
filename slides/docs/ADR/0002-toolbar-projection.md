# ADR 0002: Toolbar Projection — Renderer-Neutral OnlyOffice-Style Ribbon for slides-ui

**Status:** Accepted  
**Date:** 2026-08-09  
**Supersedes:** None  
**Related:** ADR 0001 (frozen contract), PROGRESS.md T10 (toolbar projection), T11 (acceptance tests)

---

## Summary

This ADR authorizes a **narrow, read-only extension** to the frozen public API of `slides-ui` to support a renderer-neutral, OnlyOffice-style toolbar/ribbon projection. The extension adds a `ToolbarView` with semantic DTOs (tabs, groups, controls) that **projects** existing `UiAction` capabilities — it does **not** introduce a second command model, does **not** touch `slides-api`, `slides-core`, or `slides-canvas`, and adds **zero new runtime dependencies**.

The toolbar is a **projection surface only**: it reflects what the current `EditorUi` snapshot can do via existing `UiAction` / `EditorCommandRouter`. Unsupported controls are visible but disabled with informative metadata.

---

## 1. New Public Types: `ToolbarView` and Semantic DTOs

### 1.1 `ToolbarView` — Root Projection

```rust
/// Deterministic toolbar view projected from [`EditorUi`].
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarView {
    tabs: Vec<ToolbarTab>,
    active_tab: ToolbarTabId,
}

impl ToolbarView {
    /// Tabs in stable declaration order.
    pub fn tabs(&self) -> &[ToolbarTab] { ... }

    /// Active tab identifier mirrored from [`RibbonState::active_tab`].
    pub fn active_tab(&self) -> ToolbarTabId { ... }

    /// Flattened controls in declaration order.
    pub fn controls(&self) -> Vec<&ToolbarControl> { ... }
}
```

- `tabs` is a private ordered `Vec<ToolbarTab>` accessed via `tabs()`.
- `active_tab` is a private `ToolbarTabId` (`&'static str`) accessed via `active_tab()`; it **mirrors `RibbonState.active_tab`** (not a `selected_tab` index).
- No `read_only` field on `ToolbarView`; read-only state is reflected in control `disabled`/`disabled_reason`.

### 1.2 Type Aliases (Stable ASCII IDs)

```rust
/// Stable identifier for a toolbar tab.
pub type ToolbarTabId = &'static str;
/// Stable identifier for a control group.
pub type ToolbarGroupId = &'static str;
/// Stable identifier for a control.
pub type ToolbarControlId = &'static str;
```

All IDs are `&'static str` ASCII slugs (e.g., `"home"`, `"slide-actions"`, `"new-slide"`), not enums or `String`, to allow host-side matching without `slides-ui` dependency.

### 1.3 `ToolbarTab` — Canonical Tabs (Frozen Order)

```rust
/// One toolbar tab.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarTab {
    id: ToolbarTabId,
    label: &'static str,
    accessibility_label: &'static str,
    groups: Vec<ToolbarGroup>,
}

impl ToolbarTab {
    pub fn id(&self) -> ToolbarTabId { ... }
    pub fn label(&self) -> &'static str { ... }
    pub fn accessibility_label(&self) -> &'static str { ... }
    pub fn groups(&self) -> &[ToolbarGroup] { ... }
}
```

**Canonical v1 tabs (fixed order):** `home`, `insert`, `design`, `slideshow`, `view`.  
No renaming, no insertion, no removal in v1.  
`RibbonTab` (if it exists internally) maps 1:1 to these five.  
**Transitions is not a public tab**; the sole transition control lives under **Design**, group `transitions`. Slideshow is playback-only.

### 1.4 `ToolbarGroup` — Logical Grouping Within Tab

```rust
/// One group of controls within a tab.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarGroup {
    id: ToolbarGroupId,
    label: &'static str,
    accessibility_label: &'static str,
    controls: Vec<ToolbarControl>,
}

impl ToolbarGroup {
    pub fn id(&self) -> ToolbarGroupId { ... }
    pub fn label(&self) -> &'static str { ... }
    pub fn accessibility_label(&self) -> &'static str { ... }
    pub fn controls(&self) -> &[ToolbarControl] { ... }
}
```

### 1.5 `ToolbarControl` — Atomic Actionable Unit

```rust
/// One toolbar control.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarControl {
    id: ToolbarControlId,
    kind: ToolbarControlKind,
    label: &'static str,
    accessibility_label: &'static str,
    visible: bool,
    disabled: bool,
    disabled_reason: Option<ToolbarDisabledReason>,
    action: Option<UiAction>,
}

impl ToolbarControl {
    pub fn id(&self) -> ToolbarControlId { ... }
    pub fn kind(&self) -> ToolbarControlKind { ... }
    pub fn label(&self) -> &'static str { ... }
    pub fn accessibility_label(&self) -> &'static str { ... }
    pub fn visible(&self) -> bool { ... }
    pub fn disabled(&self) -> bool { ... }
    pub fn disabled_reason(&self) -> Option<ToolbarDisabledReason> { ... }
    /// Existing [`UiAction`] associated with the control, if **enabled**.
    /// Returns `Some(&UiAction)` **only** when `disabled() == false`.
    /// Returns `None` when `disabled() == true` (regardless of `disabled_reason`).
    /// This is the **availability-safe activation rule**: host receives an action
    /// **iff** the control is enabled. The host clones the returned existing action and sends it through the existing dispatch/router boundary.
    pub fn action(&self) -> Option<&UiAction> { ... }
}
```

All fields are **private**, accessed via methods.  
`action()` returns `Option<&UiAction>` — this direct action reference is **explicitly not a second command model**; the host clones the returned existing action and sends it through the existing dispatch/router boundary.
**Availability-safe activation rule**: `action()` returns `Some(&UiAction)` **only for enabled controls** (`disabled() == false`). For disabled controls (`disabled() == true`), `action()` returns `None` regardless of `disabled_reason`. Host must call existing `dispatch` ONLY with action received from this accessor.

### 1.6 `ToolbarControlKind` — Actual Kinds (v1)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarControlKind {
    #[default] Button,
    Toggle,
    Separator,
    Label,
}
```

**Only** `Button`, `Toggle`, `Separator`, `Label` exist in v1.  
No `Dropdown`, `SplitButton`, `Gallery`, or other kinds are promised or present.

### 1.7 `ToolbarDisabledReason` — Actual Disable Gates (v1)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarDisabledReason {
    #[default] None,
    ReadOnly,
    PresentationActive,
    NotPlaying,
    EmptyDeck,
    SingleSlide,
    MissingCapability,
}
```

These are the **only** disable reasons in v1.  
`NoSelection`, `InvalidContext`, `UnsupportedOperation`, `EngineLimitation` do **not** exist.

---

## 2. What ToolbarView Is NOT

| Prohibited | Rationale |
|------------|-----------|
| **RenderPlan** | ToolbarView is UI composition, not render commands |
| **RenderCommand** | No GPU/draw coupling |
| **CanvasHostExtension** | No canvas/viewport mutation |
| **Second command model** | Activation routes exclusively through existing `UiAction` → `EditorCommandRouter` |
| **slides-api / slides-core / slides-canvas changes** | Zero touching of frozen crates |
| **Runtime `slides-pptx-adapter` dependency** | Adapter remains dev-only |

---

## 3. Projection Function: Free Function `project_toolbar`

```rust
/// Projects a [`ToolbarView`] from the current editor UI state.
///
/// The projection is pure: it reads visibility, slideshow mode, read-only
/// state, zoom state, and active ribbon tab without mutating `ui`.
pub fn project_toolbar(ui: &crate::EditorUi) -> ToolbarView { ... }
```

- **Public source API is a free function**, not an `EditorUi` method.
- `project_toolbar(&EditorUi) -> ToolbarView` — pure, read-only, deterministic.
- No mutation during projection.

---

## 4. Activation: Existing `UiAction` / `EditorCommandRouter` Only (Availability-Safe)

Host invokes actions via existing path (action is `UiAction` by value; router is the application's `EditorCommandRouter` implementation):

```rust
// CORRECT — host uses action() accessor which returns Some only for enabled controls
if let Some(action) = control.action() {
    ui.dispatch(action.clone(), router);
}

// INCORRECT — host must NOT construct UiAction manually or dispatch when action() is None
// ui.dispatch(UiAction::NewSlide, router)  // without checking control.action()
```

**No new trait, no new dispatcher, no command queue.** The toolbar is a *read-only view* of actionable operations.

**Availability-safe activation rule**: `ToolbarControl::action()` returns `Some(&UiAction)` **if and only if** `disabled() == false`. For disabled controls, `action()` returns `None`. Host MUST use the accessor; constructing `UiAction` directly bypasses the enablement gate. The existing `dispatch` takes `UiAction` by value and requires a mutable `EditorCommandRouter` reference — this is the core route boundary.

---

## 5. Enabled State Rules (v1)

Derived inside `project_toolbar` from `ui.read_only()`, `ui.slideshow.mode`, `ui.thumbnails.len()`:

| Control / Category | Enabled When | DisabledReason When Disabled |
|--------------------|--------------|------------------------------|
| **Home → NewSlide** | `!read_only && !presenting` | `ReadOnly` or `PresentationActive` |
| **Home → DeleteCurrentSlide** | `!read_only && !presenting && slide_count > 1` | `ReadOnly`, `PresentationActive`, or `SingleSlide` |
| **Home → DuplicateCurrentSlide** | `!read_only && !presenting && slide_count > 0` | `ReadOnly`, `PresentationActive`, or `EmptyDeck` |
| **Insert → InsertObject** | **never** (v1) | `MissingCapability` |
| **Design → Transitions** | **never** (v1) | `MissingCapability` |
| **Slideshow → StartSlideshow** | `!presenting && slide_count > 0` | `PresentationActive` or `EmptyDeck` |
| **Slideshow → StopSlideshow** | `presenting` | `NotPlaying` |
| **View → ZoomIn/Out/ResetZoom** | **always** | — |
| **View → ToggleProperties / ToggleNotes / ToggleFullscreen** | **always** | — |

- Local actions (zoom, panels, slideshow start/stop) are **never disabled by `read_only`**.
- Core slide actions are **disabled by `read_only`** (`DisabledReason::ReadOnly`).
- Core slide actions are **disabled while presentation mode active** (`DisabledReason::PresentationActive`).
- `StopSlideshow` disabled when not presenting → `NotPlaying`.
- Every `enabled: false` control has `Some(DisabledReason)`.

---

## 6. Canonical Tab Structure (v1 Scope — Exact)

| Tab (id) | Groups / Controls (actual v1) | Notes |
|----------|-------------------------------|-------|
| **home** | `slide-actions`: `NewSlide` (Button), `DeleteCurrentSlide` (Button), `DuplicateCurrentSlide` (Button) | Core slide actions; disabled per §5 |
| **insert** | `insert-actions`: `InsertObject` (Button, `MissingCapability`) | Single placeholder; disabled, no action |
| **design** | `transitions`: `Transitions` (Button, `MissingCapability`) | Single placeholder; disabled, no action. **No theme/size/background controls in v1.** |
| **slideshow** | `playback`: `StartSlideshow` (Button), `StopSlideshow` (Button) | Playback only — no authoring controls |
| **view** | `zoom`: `ZoomOut`, `ZoomIn`, `ResetZoom` (all Button, always enabled) | |
| | `panels`: `ToggleProperties` (Toggle), `ToggleNotes` (Toggle), `ToggleFullscreen` (Toggle) | Always enabled |

**No other controls exist in v1.** The ADR does **not** list speculative controls for Home/Insert/Design/Slideshow/View — v1 exposes only the bounded placeholder affordances shown above.

---

## 7. Dependency Freeze: No New Deps, No Runtime Adapter

| Forbidden | Status |
|-----------|--------|
| `makepad` / GPU crates in `slides-ui` | ✅ Not added |
| `serde` / serialization in `slides-ui` | ✅ Not added (DTOs are `Clone + Debug + PartialEq` only) |
| `slides-pptx-adapter` as runtime dep | ✅ Remains dev-only |
| Any new crate in workspace `Cargo.toml` | ✅ Not added |

All types live in `crates/slides-ui/src/toolbar.rs` — zero external dependencies beyond `slides-ui`'s existing `slides-core`/`slides-api` re-exports.

---

## 8. Acceptance Tests and Quality Gates

### 8.1 Unit Tests (crates/slides-ui) — Required Coverage

| Required Coverage | Description |
|------|-------------|
| `projection_is_deterministic` | Same inputs → identical `ToolbarView` (structural equality) |
| `projection_does_not_mutate_editor_ui` | `project_toolbar` called → `EditorUi` state unchanged |
| `tab_order_and_active_tab_mirror_ribbon_state` | `tabs()` order exactly `["home","insert","design","slideshow","view"]`; `active_tab()` equals `RibbonState.active_tab` |
| `all_ids_are_ascii_lowercase_alnum_hyphen` | All `id`/`label`/`accessibility_label` fields non-empty ASCII |
| `all_disabled_controls_have_reason` | Every `disabled()==true` control has `Some(DisabledReason)`; enabled controls have `None` |
| `design_transitions_disabled_missing_capability_not_in_slideshow` | Transition control in `design.transitions` is visible, disabled, `MissingCapability`, no action; **not present in `slideshow`** |
| `read_only_does_not_disable_local_view_controls` | `read_only=true` → core actions disabled; zoom/panels/slideshow remain enabled |
| `empty_deck_and_duplicate_gating` | Empty deck → `StartSlideshow` `EmptyDeck`, `DuplicateCurrentSlide` `EmptyDeck`; single slide → `DeleteCurrentSlide` `SingleSlide` |
| `slideshow_gating` | Not presenting → `StartSlideshow` enabled, `StopSlideshow` `NotPlaying`; presenting → inverse |
| `action_returns_some_iff_enabled` | `action()` returns `Some(&UiAction)` **iff** `disabled() == false`; returns `None` when `disabled() == true` for any reason |
| `disabled_read_only_returns_none_action` | Read-only mode → core actions (NewSlide, DeleteCurrentSlide, DuplicateCurrentSlide) have `action() == None` and `disabled_reason == ReadOnly` |
| `disabled_presenting_returns_none_action` | Presentation active → core actions have `action() == None` and `disabled_reason == PresentationActive` |
| `disabled_single_slide_delete_returns_none_action` | Single slide deck → DeleteCurrentSlide has `action() == None` and `disabled_reason == SingleSlide` |
| `disabled_empty_deck_duplicate_returns_none_action` | Empty deck → DuplicateCurrentSlide has `action() == None` and `disabled_reason == EmptyDeck` |
| `disabled_empty_deck_start_slideshow_returns_none_action` | Empty deck → StartSlideshow has `action() == None` and `disabled_reason == EmptyDeck` |
| `disabled_not_playing_stop_returns_none_action` | Not presenting → StopSlideshow has `action() == None` and `disabled_reason == NotPlaying` |
| `enabled_control_returns_exact_uiaction` | Every enabled control returns the **exact** `UiAction` variant that existed before (e.g., `UiAction::NewSlide`, `UiAction::ZoomIn`, `UiAction::StartSlideshow(SlideshowStart::Current)`) |
| `toggle_fullscreen_always_enabled_returns_action` | ToggleFullscreen (v1 existing always-enabled local action) returns `Some(UiAction::ToggleFullscreen)` in all states; never `None` |

### 8.2 Quality Gates (CI) — Required Before Merge

```bash
# Must pass before merge
cargo fmt --all --check
cargo clippy -p slides-ui --all-targets -- -D warnings
cargo test -p slides-ui -- toolbar  # All toolbar_* tests pass
cargo test --workspace              # No regressions
```

### 8.3 Documentation Gates

- `ToolbarView`, `ToolbarTab`, `ToolbarGroup`, `ToolbarControl`, `ToolbarControlKind`, `ToolbarDisabledReason` — all public items have `///` doc comments with examples.
- This ADR is the authoritative source for toolbar projection semantics; no separate ARCHITECTURE/GLOSSARY modifications are required by this gate.

---

## 9. Non-Goals / Explicitly Out of Scope

- **No contextual/conditional tabs** (e.g., "Table Tools" appearing only when table selected) — v1 is static canonical tabs.
- **No keyboard shortcut management** — host owns shortcuts; `accessibility_label` may include hint.
- **No localization in slides-ui** — labels are English; host localizes.
- **No theming/styling in slides-ui** — `ToolbarControlKind` is a hint only.
- **No persistence of `active_tab`** — host manages UI state; projection mirrors `RibbonState.active_tab`.
- **No `slides-pptx-adapter` runtime coupling** — adapter remains dev-only.
- **No `Dropdown`, `SplitButton`, `Gallery` control kinds** — only `Button`, `Toggle`, `Separator`, `Label`.
- **No additional `DisabledReason` variants** — only the seven defined in §1.7.

---

## 10. Verification Gates (Required Before Merge)

- `cargo fmt --all --check` — clean
- `cargo test -p slides-ui` — all tests pass (including `projection_*` / `*_toolbar*` tests)
- `cargo clippy -p slides-ui --all-targets -- -D warnings` — clean
- `cargo test --workspace` — all crates pass

---

## 11. Follow-Up

- **T11**: Acceptance test implementation for toolbar projection (unit tests per §8.1).
- **T12**: Host (Makepad) integration — `CanvasHost` consumes `ToolbarView`, renders ribbon.
- **Future ADR**: Contextual tabs, keyboard shortcut registry, localization pipeline, additional control kinds (`Dropdown`, `Gallery`), full Home/Insert/Design control coverage — only after v1 stabilization.

---

*This ADR authorizes a narrow, read-only projection surface on the frozen `slides-ui` API. Any expansion beyond the semantic DTOs defined herein, any mutation during projection, or any new dependency requires a new ADR superseding this one.*