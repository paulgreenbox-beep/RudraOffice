//! Bounded clipboard formula relocation through IronCalc's public parser and
//! model methods. Canonical input types and formatting stay in Sheets core.

use std::collections::{BTreeMap, HashMap};

use ironcalc::base::{
    Model, UserModel,
    expressions::{
        parser::{Node, new_parser_english},
        token::Error as FormulaError,
        types::{Area, CellReferenceIndex, CellReferenceRC},
    },
};
use sheets_core::{
    CellAddress, CellInput, CellRange, CellSnapshot, NumberFormat, WorkbookSnapshot,
};

use crate::{AdapterError, model_from_snapshot, native_coordinate, native_sheet_index};

const MAX_ROWS: u32 = 1_048_576;
const MAX_COLUMNS: u32 = 16_384;
const MAX_CELLS: u64 = 10_000;
const MAX_NATIVE_CUT_CELLS: u64 = 5_000;
const MAX_NATIVE_WORKBOOK_SHEETS: usize = 1_024;
const MAX_NATIVE_WORKBOOK_RECORDS: usize = 100_000;
const MAX_NATIVE_WORKBOOK_FORMULAS: usize = 10_000;
const MAX_NATIVE_WORKBOOK_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_BLANK_MODEL_NAME_BYTES: usize = 4 * 1024 * 1024;

/// Relocate formula inputs captured by a bounded rich clipboard. The source
/// cells may outlive their workbook; only sheet names and captured inputs are
/// required. Cross-workbook formulas may use local references only.
/// Formula-bearing copies cap the captured sheet context at 1,024 names and
/// 4 MiB of UTF-8 name text before constructing a native helper model.
///
/// The result contains formula inputs at destination addresses. Ordinary inputs,
/// styles, blanks, and merge metadata are preserved by the canonical caller.
#[allow(clippy::too_many_arguments)]
pub fn copied_formula_inputs(
    sheet_names: &[String],
    source_sheet_name: &str,
    source_range: &CellRange,
    source_cells: &[CellSnapshot],
    destination: CellAddress,
    destination_sheet_name: &str,
    same_workbook: bool,
) -> Result<BTreeMap<CellAddress, CellInput>, AdapterError> {
    validate_range_and_target(source_range, destination)?;
    if source_cells.len() as u64 > MAX_CELLS {
        return Err(snapshot_error(
            "clipboard contains too many materialized cells",
        ));
    }
    if source_cells
        .iter()
        .any(|cell| !source_range.contains(&cell.address))
    {
        return Err(snapshot_error(
            "clipboard cell is outside its captured range",
        ));
    }
    if !source_cells
        .iter()
        .any(|cell| matches!(cell.input, CellInput::Formula(_)))
    {
        return Ok(BTreeMap::new());
    }
    let source_index = sheet_index(sheet_names, source_sheet_name)?;
    let target_index = if same_workbook {
        sheet_index(sheet_names, destination_sheet_name)?
    } else {
        source_index
    };
    let mut model = blank_named_model(sheet_names)?;
    let mut source_parser = parser_for(sheet_names);
    let target_names = if same_workbook {
        sheet_names.to_vec()
    } else {
        vec![destination_sheet_name.to_string()]
    };
    let mut target_parser = parser_for(&target_names);
    let mut result = BTreeMap::new();

    for cell in source_cells {
        let CellInput::Formula(body) = &cell.input else {
            continue;
        };
        let target = translated_address(source_range, cell.address, destination)?;
        let source_row = native_coordinate(cell.address.row.0, "row")?;
        let source_column = native_coordinate(cell.address.column.0, "column")?;
        let target_row = native_coordinate(target.row.0, "row")?;
        let target_column = native_coordinate(target.column.0, "column")?;
        let source_context = CellReferenceRC {
            sheet: source_sheet_name.to_string(),
            row: source_row,
            column: source_column,
        };
        let old_refs = inspect_formula(
            &source_parser.parse(body, &source_context),
            !same_workbook,
            None,
            (source_row, source_column),
            0,
        )?
        .ref_errors;
        let relocated = model
            .extend_copied_value(
                &format!("={body}"),
                &CellReferenceIndex {
                    sheet: source_index,
                    row: source_row,
                    column: source_column,
                },
                &CellReferenceIndex {
                    sheet: target_index,
                    row: target_row,
                    column: target_column,
                },
            )
            .map_err(AdapterError::Engine)?;
        let target_context = CellReferenceRC {
            sheet: destination_sheet_name.to_string(),
            row: target_row,
            column: target_column,
        };
        let new_refs = inspect_formula(
            &target_parser.parse(relocated.trim_start_matches('='), &target_context),
            false,
            None,
            (target_row, target_column),
            0,
        )?
        .ref_errors;
        if new_refs > old_refs {
            return Err(snapshot_error(
                "copy would introduce a #REF! formula reference",
            ));
        }
        let input =
            CellInput::formula(relocated).map_err(|error| snapshot_error(error.to_string()))?;
        if result.insert(target, input).is_some() {
            return Err(snapshot_error("clipboard contains duplicate formula cells"));
        }
    }
    Ok(result)
}

/// Relocate formulas *within* a cut range using IronCalc's native move method.
///
/// IronCalc's public `Model::move_cell_value_to_area` cannot rewrite formulas
/// outside the moved area. Until that native dependency update is exposed, this
/// operation rejects a workbook containing any outside formula. Cross-sheet
/// moves also reject explicit references into the moved area, because the public
/// move method retains the old explicit sheet qualifier in that case. The caller
/// must keep the source intact on rejection. Before constructing a model, this
/// path applies the same 5,000-cell source and 1,024-sheet, 100,000-record,
/// 10,000-formula, 16-MiB-workbook-text caps as the `UserModel` cut path, plus
/// a 4-MiB sheet-name cap for the native helper model.
pub fn moved_formula_inputs(
    snapshot: &WorkbookSnapshot,
    source: &CellRange,
    destination: CellAddress,
) -> Result<BTreeMap<CellAddress, CellInput>, AdapterError> {
    validate_range_and_target(source, destination)?;
    if source.rows().saturating_mul(source.cols()) > MAX_NATIVE_CUT_CELLS {
        return Err(snapshot_error("native cut range exceeds 5000 cells"));
    }
    validate_native_cut_snapshot_budget(snapshot)?;
    let names: Vec<String> = snapshot
        .sheets
        .iter()
        .map(|sheet| sheet.name.clone())
        .collect();
    let source_sheet_index = snapshot
        .sheets
        .iter()
        .position(|sheet| sheet.id == source.sheet)
        .ok_or_else(|| snapshot_error("cut source sheet is missing"))?;
    let target_sheet_index = snapshot
        .sheets
        .iter()
        .position(|sheet| sheet.id == destination.sheet)
        .ok_or_else(|| snapshot_error("cut destination sheet is missing"))?;
    let source_sheet = native_sheet_index(source_sheet_index)?;
    let target_sheet = native_sheet_index(target_sheet_index)?;
    let source_name = &names[source_sheet_index];
    let target_name = &names[target_sheet_index];
    let source_row = native_coordinate(source.start.row.0, "row")?;
    let source_column = native_coordinate(source.start.column.0, "column")?;
    let area = Area {
        sheet: source_sheet,
        row: source_row,
        column: source_column,
        width: i32::try_from(source.cols())
            .map_err(|_| snapshot_error("cut width exceeds IronCalc bounds"))?,
        height: i32::try_from(source.rows())
            .map_err(|_| snapshot_error("cut height exceeds IronCalc bounds"))?,
    };
    let target_range = target_range(source, destination)?;
    if source.intersects(&target_range) {
        return Err(snapshot_error(
            "overlapping cut and paste ranges are unsupported",
        ));
    }
    let mut model = blank_named_model(&names)?;
    let mut source_parser = parser_for(&names);
    let mut target_parser = parser_for(&names);
    let mut result = BTreeMap::new();

    for sheet in &snapshot.sheets {
        for cell in &sheet.cells {
            let CellInput::Formula(body) = &cell.input else {
                continue;
            };
            if !source.contains(&cell.address) {
                return Err(snapshot_error(
                    "cut cannot update formulas outside the source range through IronCalc's public API",
                ));
            }
            let target = translated_address(source, cell.address, destination)?;
            let old_row = native_coordinate(cell.address.row.0, "row")?;
            let old_column = native_coordinate(cell.address.column.0, "column")?;
            let new_row = native_coordinate(target.row.0, "row")?;
            let new_column = native_coordinate(target.column.0, "column")?;
            let old_context = CellReferenceRC {
                sheet: source_name.clone(),
                row: old_row,
                column: old_column,
            };
            let old = inspect_formula(
                &source_parser.parse(body, &old_context),
                false,
                (source_sheet != target_sheet).then_some(&area),
                (old_row, old_column),
                0,
            )?;
            if old.explicit_inside_cut {
                return Err(snapshot_error(
                    "cross-sheet cut of an explicit reference into the moved range is unsupported",
                ));
            }
            let relocated = model
                .move_cell_value_to_area(
                    &format!("={body}"),
                    &CellReferenceIndex {
                        sheet: source_sheet,
                        row: old_row,
                        column: old_column,
                    },
                    &CellReferenceIndex {
                        sheet: target_sheet,
                        row: new_row,
                        column: new_column,
                    },
                    &area,
                )
                .map_err(AdapterError::Engine)?;
            let new_context = CellReferenceRC {
                sheet: target_name.clone(),
                row: new_row,
                column: new_column,
            };
            let new_refs = inspect_formula(
                &target_parser.parse(relocated.trim_start_matches('='), &new_context),
                false,
                None,
                (new_row, new_column),
                0,
            )?
            .ref_errors;
            if new_refs > old.ref_errors {
                return Err(snapshot_error(
                    "cut would introduce a #REF! formula reference",
                ));
            }
            let input =
                CellInput::formula(relocated).map_err(|error| snapshot_error(error.to_string()))?;
            if result.insert(target, input).is_some() {
                return Err(snapshot_error("cut contains duplicate formula cells"));
            }
        }
    }
    Ok(result)
}

/// Build an isolated IronCalc `UserModel` positioned on the source range for a
/// native same-sheet cut. The caller takes its native clipboard, selects the
/// destination, and invokes `paste_from_clipboard(..., true)` on this candidate.
/// The canonical workbook is never mutated by this helper.
///
/// IronCalc's `UserModel` cut path updates dependent formulas, including those
/// outside the source range. Its cross-sheet source context currently uses the
/// destination sheet, so that route is rejected until the native path is fixed.
/// Before allocating a native candidate, this path caps the source at 5,000
/// cells and the entire workbook at 1,024 sheets, 100,000 materialized records
/// (cells, row/column dimensions, and merges), 10,000 formulas, and 16 MiB of
/// input, style, and sheet-name text.
pub fn build_native_cut_model(
    snapshot: &WorkbookSnapshot,
    source: &CellRange,
    destination: CellAddress,
) -> Result<UserModel<'static>, AdapterError> {
    validate_native_cut(snapshot, source, destination)?;
    let names: Vec<String> = snapshot
        .sheets
        .iter()
        .map(|sheet| sheet.name.clone())
        .collect();
    let mut parser = parser_for(&names);
    for sheet in &snapshot.sheets {
        for cell in &sheet.cells {
            let CellInput::Formula(body) = &cell.input else {
                continue;
            };
            let context = CellReferenceRC {
                sheet: sheet.name.clone(),
                row: native_coordinate(cell.address.row.0, "row")?,
                column: native_coordinate(cell.address.column.0, "column")?,
            };
            inspect_formula(
                &parser.parse(body, &context),
                false,
                None,
                (context.row, context.column),
                0,
            )?;
        }
    }
    let source_sheet_index = snapshot
        .sheets
        .iter()
        .position(|sheet| sheet.id == source.sheet)
        .ok_or_else(|| snapshot_error("cut source sheet is missing"))?;
    let mut candidate = UserModel::from_model(model_from_snapshot(snapshot)?);
    candidate
        .set_selected_sheet(native_sheet_index(source_sheet_index)?)
        .map_err(AdapterError::Engine)?;
    candidate
        .set_selected_cell(
            native_coordinate(source.start.row.0, "row")?,
            native_coordinate(source.start.column.0, "column")?,
        )
        .map_err(AdapterError::Engine)?;
    candidate
        .set_selected_range(
            native_coordinate(source.start.row.0, "row")?,
            native_coordinate(source.start.column.0, "column")?,
            native_coordinate(source.end.row.0, "row")?,
            native_coordinate(source.end.column.0, "column")?,
        )
        .map_err(AdapterError::Engine)?;
    Ok(candidate)
}

/// Extract only formula changes from a successful isolated native cut/paste.
/// Moved formulas are returned at their destination, and outside formulas are
/// returned only when IronCalc actually rewrote their native text. The caller
/// retains canonical typed inputs and formatting for all other cells and owns
/// the single history transaction.
pub fn extract_native_cut_formulas(
    candidate: &UserModel<'_>,
    before: &WorkbookSnapshot,
    source: &CellRange,
    destination: CellAddress,
) -> Result<BTreeMap<CellAddress, CellInput>, AdapterError> {
    validate_native_cut(before, source, destination)?;
    let overwritten = target_range(source, destination)?;
    let baseline = model_from_snapshot(before)?;
    let names: Vec<String> = before
        .sheets
        .iter()
        .map(|sheet| sheet.name.clone())
        .collect();
    let mut parser = parser_for(&names);
    let mut changes = BTreeMap::new();
    let mut total_bytes = 0usize;
    for (sheet_index, sheet) in before.sheets.iter().enumerate() {
        let native_sheet = native_sheet_index(sheet_index)?;
        for cell in &sheet.cells {
            if !matches!(cell.input, CellInput::Formula(_)) {
                continue;
            }
            let moved = source.contains(&cell.address);
            if !moved && overwritten.contains(&cell.address) {
                continue;
            }
            let target = if moved {
                translated_address(source, cell.address, destination)?
            } else {
                cell.address
            };
            let row = native_coordinate(target.row.0, "row")?;
            let column = native_coordinate(target.column.0, "column")?;
            let after = candidate
                .get_model()
                .get_cell_formula(native_sheet, row, column)
                .map_err(AdapterError::Engine)?
                .ok_or_else(|| snapshot_error("native cut lost a surviving formula"))?;
            let before_formula = baseline
                .get_cell_formula(
                    native_sheet,
                    native_coordinate(cell.address.row.0, "row")?,
                    native_coordinate(cell.address.column.0, "column")?,
                )
                .map_err(AdapterError::Engine)?
                .ok_or_else(|| snapshot_error("native baseline lost a formula"))?;
            if !moved && after == before_formula {
                continue;
            }
            let source_context = CellReferenceRC {
                sheet: sheet.name.clone(),
                row: native_coordinate(cell.address.row.0, "row")?,
                column: native_coordinate(cell.address.column.0, "column")?,
            };
            let target_context = CellReferenceRC {
                sheet: sheet.name.clone(),
                row,
                column,
            };
            let old_refs = inspect_formula(
                &parser.parse(before_formula.trim_start_matches('='), &source_context),
                false,
                None,
                (source_context.row, source_context.column),
                0,
            )?
            .ref_errors;
            let new_refs = inspect_formula(
                &parser.parse(after.trim_start_matches('='), &target_context),
                false,
                None,
                (target_context.row, target_context.column),
                0,
            )?
            .ref_errors;
            if new_refs > old_refs {
                return Err(snapshot_error(
                    "native cut would introduce a #REF! formula reference",
                ));
            }
            total_bytes = total_bytes
                .checked_add(after.len())
                .ok_or_else(|| snapshot_error("native cut formula result is too large"))?;
            if changes.len() as u64 >= MAX_CELLS || total_bytes > 8 * 1024 * 1024 {
                return Err(snapshot_error("native cut formula result exceeds budget"));
            }
            let input =
                CellInput::formula(after).map_err(|error| snapshot_error(error.to_string()))?;
            if changes.insert(target, input).is_some() {
                return Err(snapshot_error(
                    "native cut produced duplicate formula addresses",
                ));
            }
        }
    }
    Ok(changes)
}

fn validate_native_cut(
    snapshot: &WorkbookSnapshot,
    source: &CellRange,
    destination: CellAddress,
) -> Result<(), AdapterError> {
    validate_range_and_target(source, destination)?;
    if source.rows().saturating_mul(source.cols()) > MAX_NATIVE_CUT_CELLS {
        return Err(snapshot_error("native cut range exceeds 5000 cells"));
    }
    if source.sheet != destination.sheet {
        return Err(snapshot_error(
            "native cross-sheet cut is unsupported by IronCalc's public UserModel path",
        ));
    }
    if !snapshot.sheets.iter().any(|sheet| sheet.id == source.sheet) {
        return Err(snapshot_error("cut sheet is missing"));
    }
    if source.intersects(&target_range(source, destination)?) {
        return Err(snapshot_error(
            "overlapping native cut and paste ranges are unsupported",
        ));
    }
    validate_native_cut_snapshot_budget(snapshot)?;
    Ok(())
}

/// Scan borrowed snapshot data before `model_from_snapshot` or parser setup.
/// This bounds the full-book native cut path, including its candidate clone.
fn validate_native_cut_snapshot_budget(snapshot: &WorkbookSnapshot) -> Result<(), AdapterError> {
    if snapshot.sheets.len() > MAX_NATIVE_WORKBOOK_SHEETS {
        return Err(snapshot_error("native cut workbook exceeds 1024 sheets"));
    }
    let mut record_count = 0usize;
    let mut formula_count = 0usize;
    let mut text_bytes = 0usize;
    for sheet in &snapshot.sheets {
        add_native_cut_text_bytes(&mut text_bytes, sheet.name.len())?;
        record_count = record_count
            .checked_add(sheet.cells.len())
            .and_then(|count| count.checked_add(sheet.row_dimensions.len()))
            .and_then(|count| count.checked_add(sheet.column_dimensions.len()))
            .and_then(|count| count.checked_add(sheet.merges.len()))
            .ok_or_else(|| snapshot_error("native cut workbook record count overflow"))?;
        if record_count > MAX_NATIVE_WORKBOOK_RECORDS {
            return Err(snapshot_error(
                "native cut workbook exceeds 100000 materialized records",
            ));
        }
        for cell in &sheet.cells {
            match &cell.input {
                CellInput::Text(value) => add_native_cut_text_bytes(&mut text_bytes, value.len())?,
                CellInput::Formula(value) => {
                    formula_count = formula_count
                        .checked_add(1)
                        .ok_or_else(|| snapshot_error("native cut formula count overflow"))?;
                    if formula_count > MAX_NATIVE_WORKBOOK_FORMULAS {
                        return Err(snapshot_error("native cut workbook exceeds 10000 formulas"));
                    }
                    add_native_cut_text_bytes(&mut text_bytes, value.len())?;
                }
                _ => {}
            }
            if let Some(format) = &cell.format {
                add_native_cut_text_bytes(&mut text_bytes, format.font.family.len())?;
                match &format.number_format {
                    NumberFormat::Custom(value) => {
                        add_native_cut_text_bytes(&mut text_bytes, value.len())?
                    }
                    NumberFormat::Currency { symbol, .. } => {
                        add_native_cut_text_bytes(&mut text_bytes, symbol.len())?
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn add_native_cut_text_bytes(total: &mut usize, extra: usize) -> Result<(), AdapterError> {
    *total = total
        .checked_add(extra)
        .ok_or_else(|| snapshot_error("native cut workbook text size overflow"))?;
    if *total > MAX_NATIVE_WORKBOOK_TEXT_BYTES {
        return Err(snapshot_error("native cut workbook text exceeds 16 MiB"));
    }
    Ok(())
}

fn blank_named_model(names: &[String]) -> Result<Model<'static>, AdapterError> {
    if names.len() > MAX_NATIVE_WORKBOOK_SHEETS {
        return Err(snapshot_error(
            "clipboard formula context exceeds 1024 sheets",
        ));
    }
    let mut name_bytes = 0usize;
    for name in names {
        name_bytes = name_bytes
            .checked_add(name.len())
            .ok_or_else(|| snapshot_error("clipboard sheet-name size overflow"))?;
        if name_bytes > MAX_BLANK_MODEL_NAME_BYTES {
            return Err(snapshot_error("clipboard sheet names exceed 4 MiB"));
        }
    }
    let first = names
        .first()
        .ok_or_else(|| snapshot_error("formula source workbook has no sheets"))?;
    let mut model =
        Model::new_empty("Clipboard.xlsx", "en", "UTC", "en").map_err(AdapterError::Engine)?;
    model
        .rename_sheet_by_index(0, first)
        .map_err(AdapterError::Engine)?;
    for name in names.iter().skip(1) {
        model.add_sheet(name).map_err(AdapterError::Engine)?;
    }
    Ok(model)
}

fn parser_for(names: &[String]) -> ironcalc::base::expressions::parser::Parser<'static> {
    new_parser_english(names.to_vec(), Vec::new(), HashMap::new())
}

fn sheet_index(names: &[String], name: &str) -> Result<u32, AdapterError> {
    let index = names
        .iter()
        .position(|candidate| candidate == name)
        .ok_or_else(|| snapshot_error(format!("formula sheet {name:?} is missing")))?;
    native_sheet_index(index)
}

fn validate_range_and_target(
    range: &CellRange,
    destination: CellAddress,
) -> Result<(), AdapterError> {
    range
        .validate()
        .map_err(|error| snapshot_error(error.to_string()))?;
    if range.rows().saturating_mul(range.cols()) > MAX_CELLS {
        return Err(snapshot_error(
            "clipboard formula range exceeds 10000 cells",
        ));
    }
    if range.end.row.0 >= MAX_ROWS || range.end.column.0 >= MAX_COLUMNS {
        return Err(snapshot_error(
            "clipboard source is outside IronCalc bounds",
        ));
    }
    target_range(range, destination)?;
    Ok(())
}

fn target_range(source: &CellRange, destination: CellAddress) -> Result<CellRange, AdapterError> {
    let last_row = destination
        .row
        .0
        .checked_add(
            u32::try_from(source.rows() - 1).map_err(|_| snapshot_error("invalid row count"))?,
        )
        .ok_or_else(|| snapshot_error("clipboard destination row overflow"))?;
    let last_column = destination
        .column
        .0
        .checked_add(
            u32::try_from(source.cols() - 1).map_err(|_| snapshot_error("invalid column count"))?,
        )
        .ok_or_else(|| snapshot_error("clipboard destination column overflow"))?;
    if last_row >= MAX_ROWS || last_column >= MAX_COLUMNS {
        return Err(snapshot_error(
            "clipboard destination is outside IronCalc bounds",
        ));
    }
    CellRange::new(
        destination,
        CellAddress::new(
            destination.sheet,
            sheets_core::RowIndex(last_row),
            sheets_core::ColumnIndex(last_column),
        ),
    )
    .map_err(|error| snapshot_error(error.to_string()))
}

fn translated_address(
    source_range: &CellRange,
    source: CellAddress,
    destination: CellAddress,
) -> Result<CellAddress, AdapterError> {
    if !source_range.contains(&source) {
        return Err(snapshot_error("formula source is outside clipboard range"));
    }
    let row = destination
        .row
        .0
        .checked_add(source.row.0 - source_range.start.row.0)
        .ok_or_else(|| snapshot_error("clipboard destination row overflow"))?;
    let column = destination
        .column
        .0
        .checked_add(source.column.0 - source_range.start.column.0)
        .ok_or_else(|| snapshot_error("clipboard destination column overflow"))?;
    if row >= MAX_ROWS || column >= MAX_COLUMNS {
        return Err(snapshot_error(
            "clipboard formula target is outside IronCalc bounds",
        ));
    }
    Ok(CellAddress::new(
        destination.sheet,
        sheets_core::RowIndex(row),
        sheets_core::ColumnIndex(column),
    ))
}

#[derive(Default)]
struct FormulaInspection {
    ref_errors: usize,
    explicit_inside_cut: bool,
}

impl FormulaInspection {
    fn combine(&mut self, other: Self) {
        self.ref_errors = self.ref_errors.saturating_add(other.ref_errors);
        self.explicit_inside_cut |= other.explicit_inside_cut;
    }
}

fn inspect_formula(
    node: &Node,
    cross_workbook: bool,
    cut_area: Option<&Area>,
    context: (i32, i32),
    depth: usize,
) -> Result<FormulaInspection, AdapterError> {
    if depth > 256 {
        return Err(snapshot_error("formula nesting exceeds clipboard budget"));
    }
    let inspect_child =
        |child: &Node| inspect_formula(child, cross_workbook, cut_area, context, depth + 1);
    match node {
        Node::ParseErrorKind { .. } => {
            Err(snapshot_error("unsupported or invalid clipboard formula"))
        }
        Node::WrongReferenceKind { .. } | Node::WrongRangeKind { .. } => Err(snapshot_error(
            "unresolved sheet reference in clipboard formula",
        )),
        Node::DefinedNameKind(_) | Node::TableNameKind(_) | Node::NamedVariableKind { .. } => Err(
            snapshot_error("named and structured clipboard references are unsupported"),
        ),
        Node::SpillRangeOperator { .. }
        | Node::ArrayKind(_)
        | Node::LambdaDefKind { .. }
        | Node::LambdaCallKind { .. } => Err(snapshot_error(
            "array, spill, and lambda clipboard formulas are unsupported",
        )),
        Node::NamedFunctionKind { .. } => {
            Err(snapshot_error("named clipboard functions are unsupported"))
        }
        Node::ReferenceKind {
            sheet_name,
            sheet_index,
            absolute_row,
            absolute_column,
            row,
            column,
        } => {
            if cross_workbook && sheet_name.is_some() {
                return Err(snapshot_error(
                    "cross-workbook copy cannot retain sheet references",
                ));
            }
            let explicit_inside_cut = cut_area.is_some_and(|area| {
                sheet_name.is_some()
                    && *sheet_index == area.sheet
                    && inside_area(
                        area,
                        absolute_coordinate(*row, *absolute_row, context.0),
                        absolute_coordinate(*column, *absolute_column, context.1),
                    )
            });
            Ok(FormulaInspection {
                explicit_inside_cut,
                ..FormulaInspection::default()
            })
        }
        Node::RangeKind {
            sheet_name,
            sheet_index,
            absolute_row1,
            absolute_column1,
            row1,
            column1,
            absolute_row2,
            absolute_column2,
            row2,
            column2,
        } => {
            if cross_workbook && sheet_name.is_some() {
                return Err(snapshot_error(
                    "cross-workbook copy cannot retain sheet references",
                ));
            }
            let explicit_inside_cut = cut_area.is_some_and(|area| {
                sheet_name.is_some()
                    && *sheet_index == area.sheet
                    && rectangle_overlaps_area(
                        area,
                        absolute_coordinate(*row1, *absolute_row1, context.0),
                        absolute_coordinate(*column1, *absolute_column1, context.1),
                        absolute_coordinate(*row2, *absolute_row2, context.0),
                        absolute_coordinate(*column2, *absolute_column2, context.1),
                    )
            });
            Ok(FormulaInspection {
                explicit_inside_cut,
                ..FormulaInspection::default()
            })
        }
        Node::OpRangeKind { left, right }
        | Node::OpConcatenateKind { left, right }
        | Node::OpSumKind { left, right, .. }
        | Node::OpProductKind { left, right, .. }
        | Node::OpPowerKind { left, right }
        | Node::CompareKind { left, right, .. } => {
            let mut result = inspect_child(left)?;
            result.combine(inspect_child(right)?);
            Ok(result)
        }
        Node::UnaryKind { right, .. } => inspect_child(right),
        Node::ImplicitIntersection { child, .. } => inspect_child(child),
        Node::FunctionKind { kind, args } => {
            if is_dynamic_array_constructor(&kind.to_xlsx_string()) {
                return Err(snapshot_error(
                    "dynamic array clipboard formula is unsupported",
                ));
            }
            let mut result = FormulaInspection::default();
            for argument in args {
                result.combine(inspect_child(argument)?);
            }
            Ok(result)
        }
        Node::ErrorKind(error) => Ok(FormulaInspection {
            ref_errors: usize::from(*error == FormulaError::REF),
            ..FormulaInspection::default()
        }),
        Node::BooleanKind(_) | Node::NumberKind(_) | Node::StringKind(_) | Node::EmptyArgKind => {
            Ok(FormulaInspection::default())
        }
    }
}

fn absolute_coordinate(value: i32, absolute: bool, context: i32) -> i64 {
    if absolute {
        i64::from(value)
    } else {
        i64::from(value) + i64::from(context)
    }
}

fn inside_area(area: &Area, row: i64, column: i64) -> bool {
    row >= i64::from(area.row)
        && row < i64::from(area.row) + i64::from(area.height)
        && column >= i64::from(area.column)
        && column < i64::from(area.column) + i64::from(area.width)
}

fn rectangle_overlaps_area(area: &Area, row1: i64, column1: i64, row2: i64, column2: i64) -> bool {
    row1.min(row2) < i64::from(area.row) + i64::from(area.height)
        && row1.max(row2) >= i64::from(area.row)
        && column1.min(column2) < i64::from(area.column) + i64::from(area.width)
        && column1.max(column2) >= i64::from(area.column)
}

fn is_dynamic_array_constructor(xlsx_name: &str) -> bool {
    let name = xlsx_name.strip_prefix("_xlfn.").unwrap_or(xlsx_name);
    let name = name.strip_prefix("_xlws.").unwrap_or(name);
    matches!(
        name,
        "SEQUENCE"
            | "RANDARRAY"
            | "FILTER"
            | "SORT"
            | "SORTBY"
            | "UNIQUE"
            | "TRANSPOSE"
            | "TAKE"
            | "DROP"
            | "EXPAND"
            | "CHOOSECOLS"
            | "CHOOSEROWS"
            | "HSTACK"
            | "VSTACK"
            | "TOCOL"
            | "TOROW"
            | "WRAPCOLS"
            | "WRAPROWS"
            | "TEXTSPLIT"
            | "TRIMRANGE"
            | "BYCOL"
            | "BYROW"
            | "MAP"
            | "REDUCE"
            | "SCAN"
            | "MAKEARRAY"
            | "FREQUENCY"
            | "LINEST"
            | "LOGEST"
            | "TREND"
            | "GROWTH"
            | "MINVERSE"
            | "MMULT"
            | "MUNIT"
            | "MODE.MULT"
            | "REGEXEXTRACT"
    )
}

fn snapshot_error(message: impl Into<String>) -> AdapterError {
    AdapterError::Snapshot(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sheets_core::{ColumnIndex, RowDimension, RowIndex, SheetId, SheetSnapshot};

    fn address(sheet: u32, row: u32, column: u32) -> CellAddress {
        CellAddress::new(SheetId(sheet), RowIndex(row), ColumnIndex(column))
    }

    fn range(
        sheet: u32,
        first_row: u32,
        first_column: u32,
        last_row: u32,
        last_column: u32,
    ) -> CellRange {
        CellRange::new(
            address(sheet, first_row, first_column),
            address(sheet, last_row, last_column),
        )
        .unwrap()
    }

    fn formula_cell(sheet: u32, row: u32, column: u32, body: &str) -> CellSnapshot {
        CellSnapshot {
            address: address(sheet, row, column),
            input: CellInput::formula(body).unwrap(),
            format: None,
        }
    }

    fn snapshot(sheets: Vec<SheetSnapshot>) -> WorkbookSnapshot {
        WorkbookSnapshot {
            active_sheet: sheets[0].id,
            sheets,
            ..WorkbookSnapshot::default()
        }
    }

    fn sheet(id: u32, name: &str, cells: Vec<CellSnapshot>) -> SheetSnapshot {
        SheetSnapshot {
            id: SheetId(id),
            name: name.to_string(),
            cells,
            ..SheetSnapshot::default()
        }
    }

    #[test]
    fn copy_uses_native_relative_absolute_and_mixed_references() {
        let cells = [formula_cell(1, 0, 1, "=A1+$A$1+A$1+$A1")];
        let moved = copied_formula_inputs(
            &["Sheet1".to_string()],
            "Sheet1",
            &range(1, 0, 1, 0, 1),
            &cells,
            address(1, 2, 3),
            "Sheet1",
            true,
        )
        .unwrap();
        assert_eq!(
            moved[&address(1, 2, 3)],
            CellInput::formula("=C3+$A$1+C$1+$A3").unwrap()
        );
    }

    #[test]
    fn copy_cross_sheet_keeps_explicit_sheet_and_moves_local_reference() {
        let cells = [formula_cell(1, 0, 1, "=A1+Sheet1!A1")];
        let moved = copied_formula_inputs(
            &["Sheet1".to_string(), "Sheet2".to_string()],
            "Sheet1",
            &range(1, 0, 1, 0, 1),
            &cells,
            address(2, 2, 3),
            "Sheet2",
            true,
        )
        .unwrap();
        assert_eq!(
            moved[&address(2, 2, 3)],
            CellInput::formula("=C3+Sheet1!C3").unwrap()
        );
    }

    #[test]
    fn cross_workbook_copy_accepts_local_and_rejects_explicit_sheet() {
        let names = ["Source".to_string()];
        let local = [formula_cell(1, 0, 1, "=A1*2")];
        let moved = copied_formula_inputs(
            &names,
            "Source",
            &range(1, 0, 1, 0, 1),
            &local,
            address(7, 3, 4),
            "Target",
            false,
        )
        .unwrap();
        assert_eq!(
            moved[&address(7, 3, 4)],
            CellInput::formula("=D4*2").unwrap()
        );
        let explicit = [formula_cell(1, 0, 1, "=Source!A1")];
        assert!(
            copied_formula_inputs(
                &names,
                "Source",
                &range(1, 0, 1, 0, 1),
                &explicit,
                address(7, 3, 4),
                "Target",
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn cut_moves_inside_references_and_retains_outside_references() {
        let workbook = snapshot(vec![sheet(
            1,
            "Sheet1",
            vec![formula_cell(1, 0, 1, "=A1+Z1")],
        )]);
        let moved =
            moved_formula_inputs(&workbook, &range(1, 0, 0, 0, 1), address(1, 3, 3)).unwrap();
        assert_eq!(
            moved[&address(1, 3, 4)],
            CellInput::formula("=D4+Z1").unwrap()
        );
    }

    #[test]
    fn public_native_move_rejects_observer_outside_source_area() {
        let mut model = blank_named_model(&["Sheet1".to_string()]).unwrap();
        let result = model.move_cell_value_to_area(
            "=A1",
            &CellReferenceIndex {
                sheet: 0,
                row: 1,
                column: 3,
            },
            &CellReferenceIndex {
                sheet: 0,
                row: 4,
                column: 3,
            },
            &Area {
                sheet: 0,
                row: 1,
                column: 1,
                width: 1,
                height: 1,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn native_cross_sheet_move_retains_explicit_source_qualifier() {
        let mut model = blank_named_model(&["Sheet1".to_string(), "Sheet2".to_string()]).unwrap();
        let moved = model
            .move_cell_value_to_area(
                "=Sheet1!A1",
                &CellReferenceIndex {
                    sheet: 0,
                    row: 1,
                    column: 2,
                },
                &CellReferenceIndex {
                    sheet: 1,
                    row: 4,
                    column: 5,
                },
                &Area {
                    sheet: 0,
                    row: 1,
                    column: 1,
                    width: 2,
                    height: 1,
                },
            )
            .unwrap();
        assert_eq!(moved, "=Sheet1!D4");
    }

    #[test]
    fn cross_sheet_cut_qualifies_outside_local_references() {
        let workbook = snapshot(vec![
            sheet(1, "Sheet1", vec![formula_cell(1, 0, 1, "=A1+Z1")]),
            sheet(2, "Sheet2", vec![]),
        ]);
        let moved =
            moved_formula_inputs(&workbook, &range(1, 0, 0, 0, 1), address(2, 3, 3)).unwrap();
        assert_eq!(
            moved[&address(2, 3, 4)],
            CellInput::formula("=D4+Sheet1!Z1").unwrap()
        );
    }

    #[test]
    fn cut_rejects_outside_formula_before_any_canonical_mutation() {
        let workbook = snapshot(vec![sheet(1, "Sheet1", vec![formula_cell(1, 0, 2, "=A1")])]);
        assert!(moved_formula_inputs(&workbook, &range(1, 0, 0, 0, 0), address(1, 3, 3)).is_err());
    }

    #[test]
    fn cross_sheet_cut_rejects_explicit_reference_into_source_area() {
        let workbook = snapshot(vec![
            sheet(1, "Sheet1", vec![formula_cell(1, 0, 1, "=Sheet1!A1")]),
            sheet(2, "Sheet2", vec![]),
        ]);
        assert!(moved_formula_inputs(&workbook, &range(1, 0, 0, 0, 1), address(2, 3, 3)).is_err());
    }

    #[test]
    fn cross_sheet_cut_rejects_partially_overlapping_explicit_range() {
        let workbook = snapshot(vec![
            sheet(
                1,
                "Source",
                vec![formula_cell(1, 0, 1, "=SUM(Source!A1:A10)")],
            ),
            sheet(2, "Target", vec![]),
        ]);
        let error = moved_formula_inputs(&workbook, &range(1, 0, 0, 1, 1), address(2, 3, 3))
            .err()
            .unwrap();
        assert!(error.to_string().contains("explicit reference"));
    }

    #[test]
    fn cross_sheet_cut_accepts_disjoint_explicit_range() {
        let workbook = snapshot(vec![
            sheet(
                1,
                "Source",
                vec![formula_cell(1, 0, 1, "=SUM(Source!A3:A10)")],
            ),
            sheet(2, "Target", vec![]),
        ]);
        assert!(moved_formula_inputs(&workbook, &range(1, 0, 0, 1, 1), address(2, 3, 3)).is_ok());
    }

    #[test]
    fn explicit_range_overlap_handles_reversed_endpoints() {
        let area = Area {
            sheet: 0,
            row: 1,
            column: 1,
            width: 2,
            height: 2,
        };
        assert!(rectangle_overlaps_area(&area, 10, 1, 1, 1));
        assert!(!rectangle_overlaps_area(&area, 10, 1, 3, 1));
    }

    #[test]
    fn fallback_cut_rejects_large_workbook_before_model_build() {
        let sheets = (1..=(MAX_NATIVE_WORKBOOK_SHEETS + 1))
            .map(|id| sheet(id as u32, "Sheet", vec![]))
            .collect();
        let workbook = snapshot(sheets);
        let error = moved_formula_inputs(&workbook, &range(1, 0, 0, 0, 0), address(2, 3, 3))
            .err()
            .unwrap();
        assert!(error.to_string().contains("1024 sheets"));
    }

    #[test]
    fn native_cut_candidate_selects_source_without_changing_it() {
        let workbook = snapshot(vec![sheet(1, "Sheet1", vec![formula_cell(1, 0, 0, "=B1")])]);
        let candidate =
            build_native_cut_model(&workbook, &range(1, 0, 0, 1, 1), address(1, 3, 3)).unwrap();
        let selection = candidate.get_selected_view();
        assert_eq!(selection.sheet, 0);
        assert_eq!(selection.range, [1, 1, 2, 2]);
        assert_eq!(
            candidate.get_model().get_cell_formula(0, 1, 1).unwrap(),
            Some("=B1".to_string())
        );
    }

    #[test]
    fn native_cut_extraction_includes_moved_and_dependent_formulas() {
        let workbook = snapshot(vec![sheet(
            1,
            "Sheet1",
            vec![
                formula_cell(1, 0, 0, "=B1"),
                formula_cell(1, 0, 2, "=A1"),
                formula_cell(1, 1, 2, "=Z9"),
            ],
        )]);
        let mut candidate =
            build_native_cut_model(&workbook, &range(1, 0, 0, 0, 0), address(1, 3, 3)).unwrap();
        // Stand in for the isolated native paste, whose serialized ClipboardData
        // bridge is exercised by the sheets-api integration tests.
        candidate.set_user_input(0, 4, 4, "=B1").unwrap();
        candidate.set_user_input(0, 1, 3, "=D4").unwrap();
        let result = extract_native_cut_formulas(
            &candidate,
            &workbook,
            &range(1, 0, 0, 0, 0),
            address(1, 3, 3),
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[&address(1, 3, 3)],
            CellInput::formula("=B1").unwrap()
        );
        assert_eq!(
            result[&address(1, 0, 2)],
            CellInput::formula("=D4").unwrap()
        );
    }

    #[test]
    fn native_cut_candidate_rejects_cross_sheet_path() {
        let workbook = snapshot(vec![sheet(1, "Sheet1", vec![]), sheet(2, "Sheet2", vec![])]);
        assert!(
            build_native_cut_model(&workbook, &range(1, 0, 0, 0, 0), address(2, 3, 3)).is_err()
        );
    }

    #[test]
    fn native_cut_candidate_rejects_oversized_materialized_workbook_before_build() {
        let cell = CellSnapshot {
            address: address(1, 0, 0),
            input: CellInput::Empty,
            format: None,
        };
        let workbook = snapshot(vec![sheet(
            1,
            "Sheet1",
            vec![cell; MAX_NATIVE_WORKBOOK_RECORDS + 1],
        )]);
        let error = build_native_cut_model(&workbook, &range(1, 0, 0, 0, 0), address(1, 3, 3))
            .err()
            .unwrap();
        assert!(error.to_string().contains("100000 materialized records"));
    }

    #[test]
    fn native_cut_candidate_rejects_oversized_dimensions_before_build() {
        let mut source_sheet = sheet(1, "Sheet1", vec![]);
        source_sheet.row_dimensions = vec![
            RowDimension {
                row: RowIndex(0),
                height: 20.0,
            };
            MAX_NATIVE_WORKBOOK_RECORDS + 1
        ];
        let workbook = snapshot(vec![source_sheet]);
        let error = build_native_cut_model(&workbook, &range(1, 0, 0, 0, 0), address(1, 3, 3))
            .err()
            .unwrap();
        assert!(error.to_string().contains("100000 materialized records"));
    }

    #[test]
    fn native_cut_candidate_rejects_oversized_sheet_count_before_build() {
        let sheets = (1..=(MAX_NATIVE_WORKBOOK_SHEETS + 1))
            .map(|id| sheet(id as u32, "Sheet", vec![]))
            .collect();
        let workbook = snapshot(sheets);
        let error = build_native_cut_model(&workbook, &range(1, 0, 0, 0, 0), address(1, 3, 3))
            .err()
            .unwrap();
        assert!(error.to_string().contains("1024 sheets"));
    }

    #[test]
    fn native_cut_candidate_rejects_oversized_formula_workbook_before_build() {
        let cell = formula_cell(1, 0, 0, "=1");
        let workbook = snapshot(vec![sheet(
            1,
            "Sheet1",
            vec![cell; MAX_NATIVE_WORKBOOK_FORMULAS + 1],
        )]);
        let error = build_native_cut_model(&workbook, &range(1, 0, 0, 0, 0), address(1, 3, 3))
            .err()
            .unwrap();
        assert!(error.to_string().contains("10000 formulas"));
    }

    #[test]
    fn native_cut_candidate_counts_owned_style_text_before_build() {
        let format = sheets_core::CellFormat {
            number_format: NumberFormat::Currency {
                symbol: "$".to_string(),
                precision: 2,
            },
            ..sheets_core::CellFormat::default()
        };
        let text_len = MAX_NATIVE_WORKBOOK_TEXT_BYTES - "Sheet1".len() - format.font.family.len();
        let cell = CellSnapshot {
            address: address(1, 0, 0),
            input: CellInput::Text("x".repeat(text_len)),
            format: Some(format),
        };
        let workbook = snapshot(vec![sheet(1, "Sheet1", vec![cell])]);
        let error = build_native_cut_model(&workbook, &range(1, 0, 0, 0, 0), address(1, 3, 3))
            .err()
            .unwrap();
        assert!(error.to_string().contains("text exceeds 16 MiB"));
    }

    #[test]
    fn formula_free_copy_accepts_new_destination_sheet() {
        let cells = [CellSnapshot {
            address: address(1, 0, 0),
            input: CellInput::Text("literal".to_string()),
            format: None,
        }];
        let copied = copied_formula_inputs(
            &["Old".to_string()],
            "Old",
            &range(1, 0, 0, 0, 0),
            &cells,
            address(2, 3, 3),
            "New",
            true,
        )
        .unwrap();
        assert!(copied.is_empty());
    }

    #[test]
    fn formula_free_copy_still_rejects_cell_outside_captured_range() {
        let cells = [CellSnapshot {
            address: address(1, 1, 0),
            input: CellInput::Text("literal".to_string()),
            format: None,
        }];
        assert!(
            copied_formula_inputs(
                &["Old".to_string()],
                "Old",
                &range(1, 0, 0, 0, 0),
                &cells,
                address(2, 3, 3),
                "New",
                true,
            )
            .is_err()
        );
    }
}
