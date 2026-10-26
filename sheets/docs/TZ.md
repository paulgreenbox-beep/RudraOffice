# TZ: Rudra Sheets

## Crates: sheets-api, sheets-core, sheets-grid, sheets-ui, sheets-ironcalc-adapter, sheets-xlsx
## Direction: UI -> API -> Core -> Adapter -> Engine (IronCalc)
## Reference: vendor/ironcalc (MIT), references/freecell (MIT+Apache, UI patterns only)

## MVP (TZ p14):
- cell name box, formula bar, col/row headers, virtualized grid, sheet tabs
- scrollbars, status bar, formatting panel
- Virtualization: render only visible area, sparse storage, frozen panes, variable col/row sizes
- Performance: 100k rows smooth scroll, 16-20ms per frame
- Cell types: text, number, percent, date, time, boolean, formula, empty, error
- Editing: dblclick, F2, keyboard, formula bar, Enter/Tab/Escape
- Selection: single cell, range, row, col, entire sheet, mouse, Shift, Ctrl+Shift, auto-scroll
- Formulas (IronCalc): +,-,*,/, A1 refs, absolute/relative, ranges, SUM/AVERAGE/MIN/MAX/COUNT/COUNTA/IF/AND/OR/NOT/ROUND/CONCAT, date funcs
- Errors: #DIV/0! #VALUE! #REF! #NAME? #N/A circular
- Formatting: font family/size/bold/italic/underline, color, bg, borders, alignment, wrap, number format, percent, currency, date, time, decimals
- Structure: add/delete/rename/move/duplicate sheet, insert/delete rows/cols, resize, merge/unmerge cells
- Data: sort, autofilter, search, replace, freeze first row/col/arbitrary

## Rules:
- Rust edition 2024, no external crates except std
- Each crate: Cargo.toml + src/lib.rs
- Focus on architecture, traits, types, logic
- IronCalc adapter wraps vendor/ironcalc API behind SpreadsheetEngine trait
