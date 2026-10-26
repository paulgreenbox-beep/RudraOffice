# ADR-001: TOOLBAR-001 — Effect→API Bridge для OnlyOffice-стиля Toolbar

**Статус**: Accepted (ArchCore фиксирует)
**Дата**: 2026-08-09
**Зависимости**: sheets-ui → sheets-api → sheets-core

---

## 1. Канонический ownership `FormatPatch` и `Rgba8`

**Решение**: `FormatPatch` и `Rgba8` владеют **sheets-api** (DTO для UI→API контакта). `sheets-ui` **только импортирует/реэкспортирует** их:

```rust
// sheets-ui/src/format.rs
pub use sheets_api::format::{FormatPatch, Rgba8};
```

Публичная миграция должна быть **source-compatible** для нынешнего `sheets_ui::FormatPatch`/`Rgba8` через `pub use`. Прямой зависимости `sheets-ui` от `sheets-core` нет.

---

## 2. Семантика `FormatPatch` — дельта поверх `CellFormat`

`FormatPatch` — это **дельта**. Правила:
- `None` сохраняет соответствующее значение `CellFormat`.
- Отсутствующий формат ячейки рассматривается как `CellFormat::default()`.
- **Borders V1 НЕ изменяются и не показываются в toolbar**; `merge` сохраняет `base.borders` без изменений.

```rust
// sheets-api/src/format.rs
impl FormatPatch {
    /// Мержит дельту поверх базового формата. Невалидный patch не конструируется —
    /// валидация происходит в API при построении FormatPatch (см. п. 3).
    pub fn merge_into(self, base: &CellFormat) -> CellFormat {
        let mut out = base.clone();
        if let Some(v) = self.font_family { out.font.family = v; }
        if let Some(v) = self.font_size   { out.font.size = v; }
        if let Some(v) = self.bold        { out.font.bold = v; }
        if let Some(v) = self.italic      { out.font.italic = v; }
        if let Some(v) = self.underline   { out.font.underline = v; }
        if let Some(v) = self.text_color  { out.foreground = Some(v.into()); }
        if let Some(v) = self.background  { out.background = Some(v.into()); }
        if let Some(v) = self.horizontal_alignment { out.alignment.horizontal = Some(v); }
        if let Some(v) = self.vertical_alignment   { out.alignment.vertical = Some(v); }
        if let Some(v) = self.wrap                  { out.alignment.wrap = v; }
        if let Some(v) = self.number_format         { out.number_format = v; }
        // Borders: V1 — сохраняем base.borders, не трогаем
        out
    }
}
```

Никаких `expect`/`unwrap` в merge — валидация уже пройдена в API.

---

## 3. Валидация и fallible conversion — только в API

**Вся валидация и fallible conversion выполняется в `sheets-api`, не в UI и не через `expect`.**

Отвергаются при построении `FormatPatch`:
- Пустой `font_family` (после trim).
- `font_size`: non-finite, non-positive, non-integral, > `u16::MAX`.
- Неизвестные `horizontal_alignment` / `vertical_alignment` (case-insensitive: `left|center|right`, `top|middle|bottom`).
- Malformed `number_format`.

**Каноническая строковая грамматика `number_format`** (без неявного fallback):
```
general
decimal:<0..255>
percent:<0..255>
currency:<symbol>:<0..255>
date
time
custom:<non-empty>
```

- Fixed keywords/alignment допускаются **case-insensitive** при парсинге.
- `custom:<payload>` — payload сохраняется as-is (round-trip для XLSX).
- Неизвестный формат → **ошибка валидации**, не `Custom`.

Пример API построения (псевдокод):
```rust
impl FormatPatch {
    pub fn try_from_toolbar_input(input: ToolbarFormatInput) -> Result<Self, FormatError> {
        // валидация всех полей, возврат Result
    }
}
```

---

## 4. `SpreadsheetApi::apply_format_range` — атомарный apply

```rust
// sheets-api/src/lib.rs
impl SpreadsheetApi {
    pub fn apply_format_range(
        &mut self,
        sheet: SheetId,
        range: CellRange,
        patch: FormatPatch,
        bounds: FormatPatchBounds,
    ) -> ApiResult<EngineResponse> {
        // 1. Pre-flight validation ДО изменений
        range.validate()?;                          // валидация диапазона
        if sheet != range.sheet { return Err(ApiError::CrossSheet); }
        // DocumentConfig address limits проверяются в range.validate()
        if bounds.max_cells == 0 || bounds.max_sheets == 0 {
            return Err(ApiError::InvalidBounds);
        }
        // checked multiplication
        let cell_count = range.row_count().checked_mul(range.col_count())
            .ok_or(ApiError::Overflow)?;
        if cell_count > bounds.max_cells {
            return Err(ApiError::RangeTooLarge);
        }

        // 2. Empty patch → no-op, no revision change (после validation)
        if patch.is_empty() {
            return Ok(EngineResponse::new(self.document.revision(), Vec::new()));
        }

        // 3. Materialize row-major: один ChangeSet
        let mut changes = ChangeSet::new();
        for row in range.start_row()..=range.end_row() {
            for col in range.start_col()..=range.end_col() {
                let addr = CellAddress::new(sheet, RowIndex(row), ColumnIndex(col));
                let base = self.document.cell_format(sheet, addr).cloned().unwrap_or_default();
                let merged = patch.clone().merge_into(&base);
                changes.push(Change::SetCellFormat { sheet, address: addr, format: merged });
            }
        }

        // 4. Execute ровно один раз: атомарность +1 revision при успехе, +0 при validation error
        self.execute(changes)
    }
}
```

**Важно**: Существующий `set_cell_format` остаётся **replacement** (полная замена), не мержит.

---

## 5. Effects в `SheetsViewer` — синхронный `handle_event`

**Новый public API**:
```rust
// sheets-ui/src/lib.rs
impl SheetsViewer {
    /// Синхронно потребляет UiEvent, сразу обрабатывает все UiEffect.
    /// UI не отдаётся mutable. ВНЕШНИХ deferred apply_effect НЕТ в V1.
    pub fn handle_event(&mut self, event: UiEvent) -> UiResult<()> {
        // 1. Генерирует Vec<UiEffect> из event (внутри SheetsUi)
        // 2. Немедленно диспатчит каждый эффект:
        for effect in self.ui.take_effects() {
            self.dispatch_effect(effect)?;
        }
        Ok(())
    }

    // Private dispatcher для тестируемости
    fn dispatch_effect(&mut self, effect: UiEffect) -> UiResult<()> {
        match effect {
            UiEffect::ApplyFormat { range, patch } => {
                // persistent effect требует Ready + has_document
                if !self.api.has_document() { return Err(UiError::NoDocument); }
                let bounds = FormatPatchBounds { max_cells: 10_000, max_sheets: 1 };
                self.api.apply_format_range(range.sheet, range, patch, bounds)?;
                // после успеха: fresh snapshot, монотонно большая UI revision
                self.refresh_snapshot()?;
            }
            UiEffect::SetFrozenPanes(panes) => {
                // Local only — не трогает document revision
                self.ui.set_frozen_panes(panes);
            }
            UiEffect::CommitCellRaw { address, raw } => { /* ... */ }
            UiEffect::ClearRange(range) => { /* ... */ }
            UiEffect::ActivateSheet(sheet) => { /* ... */ }
            UiEffect::AddSheet(name) => { /* ... */ }
            UiEffect::RenameSheet { sheet, name } => { /* ... */ }
            UiEffect::DeleteSheet(sheet) => { /* ... */ }
            UiEffect::DuplicateSheet { sheet, name } => { /* ... */ }
            UiEffect::MoveSheet { sheet, target_index } => { /* ... */ }
            // Каждая ветка явна, БЕЗ `_` catch-all
        }
        Ok(())
    }

    fn refresh_snapshot(&mut self) -> UiResult<()> {
        let snap = self.build_snapshot()?;
        self.ui.sync(snap);
        Ok(())
    }
}
```

**Правила**:
- **Нет `generation` полей в `UiEffect`/`UiSnapshot` в V1**. Stale queued effects исключены синхронным потреблением.
- Persistent effects требуют `Ready + has_document`; после `close`/`reopen` старый внешний effect невозможно передать viewer.
- Local `SetFrozenPanes` не меняет document revision.
- Range-based эффекты маппятся на API **атомарно** (один execute).
- После persist success: fresh snapshot, монотонно большая UI revision.
- API error сохраняет document и Ready, refreshes status Error **без auto retry**.

---

## 6. V1 Scope: OnlyOffice toolbar, renderer-neutral

**Constraints**:
- ❌ No Makepad dependencies/widgets в workspace.
- ✅ Renderer-agnostic: `sheets-ui` выдает `UiFrame` + `UiEffect` — любой рендерер потребляет.
- ✅ Toolbar = источник `UiEvent::ApplyFormat(patch)` + `UiEvent::SetFrozenPanes`.
- ✅ `FormatPatch` — plain Rust struct (serializable, no lifetimes).

**V1 Toolbar MVP fields** (map 1:1 к `FormatPatch`):
| Toolbar control | `FormatPatch` field |
|----------------|---------------------|
| Font family dropdown | `font_family` |
| Font size spinner | `font_size` |
| Bold/Italic/Underline toggles | `bold`/`italic`/`underline` |
| Text color picker | `text_color` |
| Fill color picker | `background` |
| H-align: Left/Center/Right | `horizontal_alignment` |
| V-align: Top/Middle/Bottom | `vertical_alignment` |
| Wrap text toggle | `wrap` |
| Number format dropdown | `number_format` |
| Panel toggle (responsive/overflow data model) | — |

**Out of V1 scope** (явно исключены):
- Undo/Redo/Cut/Copy/Paste/Open/Save
- Merge cells UI
- Borders UI
- Любые Makepad-виджеты

---

## 7. Borders: Technical Debt & Safe V1 Solution

- `CellFormat.borders` (core) — полная поддержка.
- `FormatPatch` **не получает borders поля** в V1.
- `merge_into` **сохраняет** `base.borders` без изменений.
- Toolbar **не показывает** borders контролы (hidden/disabled).
- XLSX import сохраняет borders → `CellView` → read-only render.
- **ADR-002** (follow-up) введет `BorderPatch` + borders UI когда понадобится.

**Почему безопасно**: `SetCellFormat` в core заменяет весь `CellFormat`, но `merge_into` читает `base` → мержит дельту → пишет полный `CellFormat` с сохранёнными borders. Нет потери данных при повторных форматированиях.

---

## 8. Tests / Gates (Acceptance Criteria)

| ID | Check |
|----|-------|
| T5 | Compile fixture baseline first |
| T5.1 | API: merge semantics (delta, None preserves, default base) |
| T5.2 | API: no-op empty patch returns current revision, no transaction |
| T5.3 | API: bounds/range validation (max_cells, max_sheets, cross-sheet, overflow) |
| T5.4 | API: 2×2 range produces exactly one revision increment |
| T5.5 | API: invalid values rejected (empty font, bad size, bad align, malformed number-format) |
| T6 | Viewer: no document → error; close/reopen → old effect rejected |
| T6.1 | Viewer: success persist → fresh snapshot, monotonically larger UI revision |
| T6.2 | Viewer: local SetFrozenPanes doesn't touch document revision |
| T7 | Layout: responsive deterministic, no overlap, disabled without selection |
| Gates | `cargo fmt --check && cargo check && cargo test && cargo clippy -- -D warnings`, no new deps |

---

## 9. Follow-ups (out of V1 scope)

- **ADR-002**: `BorderPatch` + borders toolbar (requires `FormatPatch` extension)
- **ADR-003**: Batched large-range formatting (progress, cancellation)
- **ADR-004**: Conditional formatting / named styles
- **ADR-005**: Format painter (copy/paste format between ranges)

---

**Примечание**: Этот ADR описывает **замороженный архитектурный контракт**. Реализация — в отдельных тикетах. ArchCore зафиксировал решение; код не меняется в этом ответе.