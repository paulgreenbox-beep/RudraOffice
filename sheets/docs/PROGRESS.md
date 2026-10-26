# PROGRESS.md — Final Report

> **Session**: RudraOffice / sheets — Rust 2024 std-only architecture  
> **Status**: COMPLETE — All verification gates PASS

---
## Update — 2026-08-05

This update supersedes any conflicting historical baseline statements below.

- The production spreadsheet path uses the vendored **IronCalc 0.8.3** engine for formula evaluation and real `.xlsx` package import/export.
- Direct cell formatting round-trips through IronCalc: font family/size, bold/italic/underline, foreground and fill colours, four borders, alignment, wrapping, and General/decimal/percent/currency/date/time/custom number formats.
- A formatted empty cell is preserved as a real cell, not discarded during package round-trip.
- The Writer-facing canonical model still does **not** represent inherited row/column styles, named styles, or conditional formatting; these cases must not be described as lossless.
- Verification: `cargo test --workspace --locked` — **111 passed, 0 failed**.

## 1. Architecture Overview

```
sheets-ui -> sheets-api -> sheets-core -> sheets-ironcalc-adapter -> engine
  |           |              |                  |
  |           |              |                  +-- IronCalc engine bridge
  |           |              +-- std-only core: Workbook, CellAddress, CellRange,
  |           |                  CellInput, ChangeSet, SpreadsheetEngine, validation
  |           +-- public API: generic SpreadsheetApi<T> facade, CellView (display_text, raw_text),
  |               DocumentConfig, WorkbookSnapshot
  +-- renderer-neutral: UiEvent, UiEffect, UiFrame, selection, drag,
      viewport, editor, formula lifecycle, frozen panes, A1 box, scrolling

Supporting crates:
  sheets-grid   — renderer-neutral geometry (layout, viewport, frozen panes)
  sheets-xlsx   — codec supporting crates (import/export)
```

- **Rust Edition**: 2024
- **Dependencies**: no third-party crates; std plus internal workspace crate dependencies; UI direct deps sheets-api/sheets-grid
- **Workspace**: 6 crates — `sheets-core`, `sheets-api`, `sheets-grid`, `sheets-ui`, `sheets-ironcalc-adapter`, `sheets-xlsx`

---

## 2. Baseline State (Before Work)

| Metric | Value |
|--------|-------|
| API compilation errors | **26** |
| Root causes | Missing imports of core `ChangeSet` / `SpreadsheetEngine`; conflicting `Debug` impl; illegal `Display`/inherent impls on external `CellInput` |
| Test coverage | API test target blocked by compilation |

---

## 3. API Layer (sheets-api) — Fixes & Tests

### 3.1 Errors Resolved (26 → 0)

| # | Error Category | Resolution |
|---|----------------|------------|
| 1–8 | Missing `ChangeSet` / `SpreadsheetEngine` imports | Canonical imports from `sheets-core` |
| 9–14 | Conflicting `Debug` impl on public types | Removed duplicate; single canonical derive |
| 15–26 | Illegal `Display`/inherent impls on external `CellInput` | Local private formatting/conversion helpers; public `CellView` with `display_text` / `raw_text` |

### 3.2 Key Functional Changes

- `DocumentConfig` defaults: 100,000 rows × 16,384 cols; row height 24, col width 96
- Generic `SpreadsheetApi<T>` retained
- Private workbook access → public methods + `WorkbookSnapshot`
- Empty `ChangeSet` short-circuit in apply path
- Safe Text/Formula literal replacement: case-sensitive and ASCII-case-insensitive replacement is UTF-8-boundary safe; preserves Text/Formula types and replaces all applicable non-overlapping matches
- Filter row grouping fixed
- Sheet/address validation

### 3.3 API Test Suite — **14 tests** (all passing)

Coverage: defaults, transaction/revision, entry parsing, deterministic search, formatting bounds, multi-row filter, cell-view sheet validation/display/raw, no-op/case-sensitive/case-insensitive/UTF-8 replacement.

---

## 4. UI Layer (sheets-ui) — Renderer-Neutral Completion (all in `src/lib.rs`)

### 4.1 Features Completed

| Feature | Details |
|---------|---------|
| **UiEvent / UiEffect / UiFrame** | Renderer-neutral event/effect/frame types |
| **Default SheetId** | `SheetId(1)` |
| **UiSnapshot** | Revision + stale/duplicate/active metadata validation |
| **VirtualGrid** | Sparse sync, bounded frame |
| **Viewport hit testing** | Local coordinate → cell hit |
| **Selection** | Drag extend, shift range, A1 range box |
| **Keyboard selection** | Arrow/Shift/Ctrl navigation + ensure-visible scrolling |
| **Formula/Editor lifecycle** | `begin` / `commit` / `cancel` / raw text |
| **TextInput** | Sole insertion path |
| **Editing** | Delete / Backspace |
| **Constrained layout** | Small viewport handling |
| **Frozen panes** | Top/left state + effect |

### 4.2 UI Test Suite — **51 tests** (all passing)

Coverage: snapshot validation; virtual grid sync; hit testing; selection drag/shift/A1/keyboard; editor lifecycle; text input/editing; layout constraints; frozen pane state.

---

## 5. Core / Grid / Adapter / XLSX — Minor & Baseline

- **sheets-core**: PI literal cleanup in test-only code
- **sheets-grid**: 12 tests passing (layout, viewport, frozen)
- **sheets-ironcalc-adapter**: 7 tests passing (engine bridge)
- **sheets-xlsx**: 11 tests passing (import/export)

---

## 6. Final Verification — **ALL GATES PASS**

```bash
# Formatting
cargo fmt --all -- --check
# -> OK: all files formatted

# Compilation (all targets: lib, bin, test, bench, doc)
cargo check --workspace --all-targets --locked
# -> OK: 0 errors, 0 warnings

# Unit Tests (workspace)
cargo test --workspace --locked
# -> OK: 109 tests total, 0 failed
#       sheets-api:        14 tests
#       sheets-core:       14 tests
#       sheets-grid:       12 tests
#       sheets-ironcalc-adapter: 7 tests
#       sheets-ui:         51 tests
#       sheets-xlsx:       11 tests

# Clippy (deny warnings)
cargo clippy --workspace --all-targets --locked -- -D warnings
# -> OK: 0 warnings

# Release Build
cargo build --workspace --release --locked
# -> OK: artifacts in target/release/
```

**Summary**: 109 unit tests passing (14 + 14 + 12 + 7 + 51 + 11), 0 failed, 0 warnings, clean release build.

---

## 7. Deferred / Explicitly Unsupported

| Feature | Reason |
|---------|--------|
| `apply_sorted_rows` → `ApiUnsupportedError::SortApply` | Explicit unsupported return |
| Native import/export codec backend | Unsupported XLSX backend via `UnsupportedPartsCodec` and `NativeImport` category |

---

## 8. Changed Files

| Crate | File |
|-------|------|
| sheets-api | `crates/sheets-api/src/lib.rs` |
| sheets-ui | `crates/sheets-ui/src/lib.rs` |
| sheets-core | `crates/sheets-core/src/lib.rs` (test-only PI literal cleanup) |

---

## End of Report — `PROGRESS.md` is the single source of truth for this milestone.