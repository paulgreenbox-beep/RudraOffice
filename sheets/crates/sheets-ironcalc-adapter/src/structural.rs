//! Formula-aware row and column edits on an isolated IronCalc candidate.
//!
//! IronCalc owns reference displacement. The canonical input types remain owned
//! by Sheets: reading ordinary values back from IronCalc would turn dates,
//! times, and percentages into plain numbers and would lose literal text intent.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ironcalc::base::expressions::{
    parser::{Node, new_parser_english},
    types::CellReferenceRC,
};
use sheets_core::{CellAddress, CellInput, ColumnIndex, RowIndex, SheetId, WorkbookSnapshot};

use crate::{AdapterError, model_from_snapshot, native_coordinate, native_sheet_index};

const MAX_ROWS: u32 = 1_048_576;
const MAX_COLUMNS: u32 = 16_384;

/// One contiguous, zero-based structural change. The candidate is never
/// committed by this adapter; the caller owns the canonical transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuralOperation {
    InsertRows {
        sheet: SheetId,
        at: RowIndex,
        count: u32,
    },
    DeleteRows {
        sheet: SheetId,
        start: RowIndex,
        count: u32,
    },
    InsertColumns {
        sheet: SheetId,
        at: ColumnIndex,
        count: u32,
    },
    DeleteColumns {
        sheet: SheetId,
        start: ColumnIndex,
        count: u32,
    },
}

impl StructuralOperation {
    fn parts(self) -> (SheetId, bool, bool, u32, u32) {
        match self {
            Self::InsertRows { sheet, at, count } => (sheet, true, true, at.0, count),
            Self::DeleteRows {
                sheet,
                start,
                count,
            } => (sheet, true, false, start.0, count),
            Self::InsertColumns { sheet, at, count } => (sheet, false, true, at.0, count),
            Self::DeleteColumns {
                sheet,
                start,
                count,
            } => (sheet, false, false, start.0, count),
        }
    }

    fn validate(self, snapshot: &WorkbookSnapshot) -> Result<(u32, i32, i32), AdapterError> {
        let (sheet, rows, _, at, count) = self.parts();
        let limit = if rows { MAX_ROWS } else { MAX_COLUMNS };
        let end = at.checked_add(count).ok_or_else(|| {
            AdapterError::Snapshot("structural coordinate addition overflow".into())
        })?;
        if count == 0 || at >= limit || end > limit {
            return Err(AdapterError::Snapshot(format!(
                "structural {} range is outside IronCalc bounds",
                if rows { "row" } else { "column" }
            )));
        }
        let index = snapshot
            .sheets
            .iter()
            .position(|candidate| candidate.id == sheet)
            .ok_or_else(|| AdapterError::Snapshot("structural target sheet is missing".into()))?;
        if snapshot.sheets[index + 1..]
            .iter()
            .any(|candidate| candidate.id == sheet)
        {
            return Err(AdapterError::Snapshot(
                "structural target sheet identifier is duplicated".into(),
            ));
        }
        let native_sheet = native_sheet_index(index)?;
        let native_at = native_coordinate(at, if rows { "row" } else { "column" })?;
        let native_count = i32::try_from(count).map_err(|_| {
            AdapterError::Snapshot("structural count exceeds IronCalc bounds".into())
        })?;
        Ok((native_sheet, native_at, native_count))
    }

    fn map_address(self, address: CellAddress) -> Result<Option<CellAddress>, AdapterError> {
        let (sheet, rows, insert, at, count) = self.parts();
        if address.sheet != sheet {
            return Ok(Some(address));
        }
        let index = if rows {
            address.row.0
        } else {
            address.column.0
        };
        let limit = if rows { MAX_ROWS } else { MAX_COLUMNS };
        let mapped = if index < at {
            Some(index)
        } else if insert {
            Some(index.checked_add(count).ok_or_else(|| {
                AdapterError::Snapshot("structural coordinate addition overflow".into())
            })?)
        } else if index < at + count {
            None
        } else {
            Some(index - count)
        };
        let Some(mapped) = mapped else {
            return Ok(None);
        };
        if mapped >= limit {
            return Err(AdapterError::Snapshot(
                "structural edit would move an input beyond IronCalc bounds".into(),
            ));
        }
        Ok(Some(if rows {
            CellAddress::new(sheet, RowIndex(mapped), address.column)
        } else {
            CellAddress::new(sheet, address.row, ColumnIndex(mapped))
        }))
    }
}

/// Returns every surviving canonical input at its candidate address, across
/// all sheets. Ordinary input variants are copied from the source exactly;
/// only formulas are read from IronCalc after its native structural mutation.
/// A style-only cell remains `CellInput::Empty` for the caller to pair with its
/// separately shifted canonical format.
pub fn structural_inputs(
    snapshot: &WorkbookSnapshot,
    operation: &StructuralOperation,
) -> Result<BTreeMap<CellAddress, CellInput>, AdapterError> {
    let (native_sheet, native_at, native_count) = operation.validate(snapshot)?;
    let (_, _, insert, _, _) = operation.parts();
    let mut parser = new_parser_english(
        snapshot
            .sheets
            .iter()
            .map(|sheet| sheet.name.clone())
            .collect(),
        Vec::new(),
        HashMap::new(),
    );
    let mut seen = BTreeSet::new();
    // Validate every formula, including formulas on other sheets: IronCalc's
    // structural primitive walks all formula cells in the workbook.
    for sheet in &snapshot.sheets {
        for cell in &sheet.cells {
            if cell.address.sheet != sheet.id
                || cell.address.row.0 >= MAX_ROWS
                || cell.address.column.0 >= MAX_COLUMNS
                || !seen.insert(cell.address)
            {
                return Err(AdapterError::Snapshot(
                    "structural source contains an invalid or duplicate cell address".into(),
                ));
            }
            let survives = operation.map_address(cell.address)?.is_some();
            let CellInput::Formula(body) = &cell.input else {
                continue;
            };
            let context = CellReferenceRC {
                sheet: sheet.name.clone(),
                row: native_coordinate(cell.address.row.0, "row")?,
                column: native_coordinate(cell.address.column.0, "column")?,
            };
            validate_formula(
                &parser.parse(body.trim_start_matches('='), &context),
                !insert && survives,
                0,
            )
            .map_err(|reason| {
                AdapterError::Snapshot(format!(
                    "cannot structurally edit formula at row {}, column {} on sheet {}: {reason}",
                    context.row, context.column, sheet.name
                ))
            })?;
        }
    }

    let mut model = model_from_snapshot(snapshot)?;
    // Comparing native formula text before and after avoids rewriting unrelated
    // canonical formula text solely because IronCalc canonicalizes its display.
    let mut native_before = BTreeMap::new();
    for (sheet_index, sheet) in snapshot.sheets.iter().enumerate() {
        let index = native_sheet_index(sheet_index)?;
        for cell in &sheet.cells {
            if matches!(cell.input, CellInput::Formula(_)) {
                let formula = model
                    .get_cell_formula(
                        index,
                        native_coordinate(cell.address.row.0, "row")?,
                        native_coordinate(cell.address.column.0, "column")?,
                    )
                    .map_err(AdapterError::Engine)?
                    .ok_or_else(|| {
                        AdapterError::Snapshot("source formula is missing in IronCalc".into())
                    })?;
                native_before.insert(cell.address, formula);
            }
        }
    }
    let native_result = match *operation {
        StructuralOperation::InsertRows { .. } => {
            model.insert_rows(native_sheet, native_at, native_count)
        }
        StructuralOperation::DeleteRows { .. } => {
            model.delete_rows(native_sheet, native_at, native_count)
        }
        StructuralOperation::InsertColumns { .. } => {
            model.insert_columns(native_sheet, native_at, native_count)
        }
        StructuralOperation::DeleteColumns { .. } => {
            model.delete_columns(native_sheet, native_at, native_count)
        }
    };
    native_result.map_err(AdapterError::Engine)?;

    let mut candidate = BTreeMap::new();
    for (sheet_index, sheet) in snapshot.sheets.iter().enumerate() {
        let index = native_sheet_index(sheet_index)?;
        for cell in &sheet.cells {
            let Some(address) = operation.map_address(cell.address)? else {
                continue;
            };
            let input = match &cell.input {
                CellInput::Formula(_) => {
                    let formula = model
                        .get_cell_formula(
                            index,
                            native_coordinate(address.row.0, "row")?,
                            native_coordinate(address.column.0, "column")?,
                        )
                        .map_err(AdapterError::Engine)?
                        .ok_or_else(|| {
                            AdapterError::Snapshot(
                                "structurally moved formula is missing in IronCalc".into(),
                            )
                        })?;
                    let context = CellReferenceRC {
                        sheet: sheet.name.clone(),
                        row: native_coordinate(address.row.0, "row")?,
                        column: native_coordinate(address.column.0, "column")?,
                    };
                    validate_formula(
                        &parser.parse(formula.trim_start_matches('='), &context),
                        false,
                        0,
                    )
                    .map_err(|reason| {
                        AdapterError::Snapshot(format!(
                            "invalid structurally rewritten formula on sheet {}: {reason}",
                            sheet.name
                        ))
                    })?;
                    if native_before.get(&cell.address) == Some(&formula) {
                        cell.input.clone()
                    } else {
                        CellInput::formula(formula)
                            .map_err(|error| AdapterError::Snapshot(error.to_string()))?
                    }
                }
                literal => literal.clone(),
            };
            if candidate.insert(address, input).is_some() {
                return Err(AdapterError::Snapshot(
                    "structural candidate has duplicate destination addresses".into(),
                ));
            }
        }
    }
    Ok(candidate)
}

fn validate_formula(
    node: &Node,
    deleting_surviving_formula: bool,
    depth: usize,
) -> Result<(), &'static str> {
    if depth > 256 {
        return Err("formula nesting exceeds structural edit budget");
    }
    match node {
        Node::ParseErrorKind { .. } => Err("unsupported or invalid formula syntax"),
        Node::WrongReferenceKind { .. } | Node::WrongRangeKind { .. } => {
            Err("unresolved sheet reference")
        }
        Node::DefinedNameKind(_)
        | Node::TableNameKind(_)
        | Node::NamedVariableKind { .. }
        | Node::NamedFunctionKind { .. } => Err("named or structured reference is unsupported"),
        Node::SpillRangeOperator { .. }
        | Node::ArrayKind(_)
        | Node::LambdaDefKind { .. }
        | Node::LambdaCallKind { .. } => Err("array, spill or lambda formula is unsupported"),
        // A dynamic range can hide a deleted boundary in either operand.
        Node::OpRangeKind { .. } if deleting_surviving_formula => {
            Err("dynamic range cannot be safely rebased after deletion")
        }
        Node::OpRangeKind { left, right }
        | Node::OpConcatenateKind { left, right }
        | Node::OpSumKind { left, right, .. }
        | Node::OpProductKind { left, right, .. }
        | Node::OpPowerKind { left, right }
        | Node::CompareKind { left, right, .. } => {
            validate_formula(left, deleting_surviving_formula, depth + 1)?;
            validate_formula(right, deleting_surviving_formula, depth + 1)
        }
        Node::UnaryKind { right, .. } => {
            validate_formula(right, deleting_surviving_formula, depth + 1)
        }
        Node::ImplicitIntersection { child, .. } => {
            validate_formula(child, deleting_surviving_formula, depth + 1)
        }
        Node::FunctionKind { kind, args } => {
            if is_dynamic_array_constructor_name(&kind.to_xlsx_string()) {
                return Err("dynamic array formula is unsupported");
            }
            args.iter().try_for_each(|argument| {
                validate_formula(argument, deleting_surviving_formula, depth + 1)
            })
        }
        Node::BooleanKind(_)
        | Node::NumberKind(_)
        | Node::StringKind(_)
        | Node::ReferenceKind { .. }
        | Node::RangeKind { .. }
        | Node::ErrorKind(_)
        | Node::EmptyArgKind => Ok(()),
    }
}

fn is_dynamic_array_constructor_name(xlsx_name: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use sheets_core::{CellSnapshot, SheetSnapshot};

    fn sheet(id: u32, name: &str, cells: Vec<(u32, u32, CellInput)>) -> SheetSnapshot {
        let sheet_id = SheetId(id);
        SheetSnapshot {
            id: sheet_id,
            name: name.into(),
            cells: cells
                .into_iter()
                .map(|(row, column, input)| CellSnapshot {
                    address: CellAddress::new(sheet_id, RowIndex(row), ColumnIndex(column)),
                    input,
                    format: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn workbook(sheets: Vec<SheetSnapshot>) -> WorkbookSnapshot {
        WorkbookSnapshot {
            active_sheet: sheets[0].id,
            sheets,
            ..Default::default()
        }
    }

    fn address(sheet: u32, row: u32, column: u32) -> CellAddress {
        CellAddress::new(SheetId(sheet), RowIndex(row), ColumnIndex(column))
    }

    fn formula(body: &str) -> CellInput {
        CellInput::formula(body).expect("valid fixture formula")
    }

    #[test]
    fn insert_row_moves_inputs_and_rewrites_relative_absolute_and_mixed_references() {
        let source = workbook(vec![sheet(
            1,
            "Sheet1",
            vec![
                (0, 0, CellInput::Number(10.0)),
                (1, 0, CellInput::Number(20.0)),
                (0, 1, formula("=A1+$A$2+A$2+$A2")),
                (1, 1, formula("=A2")),
            ],
        )]);
        let result = structural_inputs(
            &source,
            &StructuralOperation::InsertRows {
                sheet: SheetId(1),
                at: RowIndex(1),
                count: 1,
            },
        )
        .expect("insert row");
        assert_eq!(
            result.get(&address(1, 0, 0)),
            Some(&CellInput::Number(10.0))
        );
        assert_eq!(
            result.get(&address(1, 2, 0)),
            Some(&CellInput::Number(20.0))
        );
        assert_eq!(
            result.get(&address(1, 0, 1)),
            Some(&formula("=A1+$A$3+A$3+$A3"))
        );
        assert_eq!(result.get(&address(1, 2, 1)), Some(&formula("=A3")));
        assert!(!result.contains_key(&address(1, 1, 0)));
    }

    #[test]
    fn insert_row_rewrites_ranges_and_other_sheet_references() {
        let source = workbook(vec![
            sheet(
                1,
                "Данные 2",
                vec![
                    (1, 0, CellInput::Number(7.0)),
                    (0, 1, formula("=SUM(A1:A3)")),
                ],
            ),
            sheet(
                2,
                "Summary",
                vec![(0, 0, formula("='Данные 2'!$A2+\"A2\""))],
            ),
        ]);
        let result = structural_inputs(
            &source,
            &StructuralOperation::InsertRows {
                sheet: SheetId(1),
                at: RowIndex(1),
                count: 1,
            },
        )
        .expect("insert row");
        assert_eq!(result.get(&address(1, 0, 1)), Some(&formula("=SUM(A1:A4)")));
        assert_eq!(
            result.get(&address(2, 0, 0)),
            Some(&formula("='Данные 2'!$A3+\"A2\""))
        );
    }

    #[test]
    fn delete_row_keeps_native_ref_error_and_shrinks_surviving_range() {
        let source = workbook(vec![sheet(
            1,
            "Sheet1",
            vec![
                (1, 0, CellInput::Number(20.0)),
                (0, 1, formula("=A2")),
                (0, 2, formula("=SUM(A1:A3)")),
            ],
        )]);
        let result = structural_inputs(
            &source,
            &StructuralOperation::DeleteRows {
                sheet: SheetId(1),
                start: RowIndex(1),
                count: 1,
            },
        )
        .expect("delete row");
        assert_eq!(result.get(&address(1, 0, 1)), Some(&formula("=#REF!")));
        assert_eq!(result.get(&address(1, 0, 2)), Some(&formula("=SUM(A1:A2)")));
        assert!(!result.contains_key(&address(1, 1, 0)));
    }

    #[test]
    fn deleted_range_endpoints_shrink_moved_mixed_and_reversed_formulas() {
        let source = workbook(vec![sheet(
            1,
            "Sheet1",
            vec![
                (4, 1, formula("=SUM($A$1:A$3)")),
                (4, 2, formula("=SUM(B3:A1)")),
            ],
        )]);
        let before = source.clone();
        for deleted_row in [0, 2] {
            let result = structural_inputs(
                &source,
                &StructuralOperation::DeleteRows {
                    sheet: SheetId(1),
                    start: RowIndex(deleted_row),
                    count: 1,
                },
            )
            .expect("delete range endpoint");
            assert_eq!(
                result.get(&address(1, 3, 1)),
                Some(&formula("=SUM($A$1:A$2)"))
            );
            assert_eq!(result.get(&address(1, 3, 2)), Some(&formula("=SUM(A1:B2)")));
        }
        assert_eq!(source, before);

        let column_source = workbook(vec![sheet(
            1,
            "Sheet1",
            vec![(0, 4, formula("=SUM(A1:C1)"))],
        )]);
        let result = structural_inputs(
            &column_source,
            &StructuralOperation::DeleteColumns {
                sheet: SheetId(1),
                start: ColumnIndex(2),
                count: 1,
            },
        )
        .expect("delete column range endpoint");
        assert_eq!(result.get(&address(1, 0, 3)), Some(&formula("=SUM(A1:B1)")));
    }

    #[test]
    fn foreign_range_endpoint_shrinks_after_target_sheet_deletion() {
        let source = workbook(vec![
            sheet(1, "Данные 2", Vec::new()),
            sheet(
                2,
                "Summary",
                vec![(0, 0, formula("=SUM('Данные 2'!A1:A3)"))],
            ),
        ]);
        let result = structural_inputs(
            &source,
            &StructuralOperation::DeleteRows {
                sheet: SheetId(1),
                start: RowIndex(2),
                count: 1,
            },
        )
        .expect("foreign range endpoint");
        assert_eq!(
            result.get(&address(2, 0, 0)),
            Some(&formula("=SUM('Данные 2'!A1:A2)"))
        );
    }

    #[test]
    fn fully_deleted_range_becomes_a_native_ref_error() {
        let source = workbook(vec![sheet(
            1,
            "Sheet1",
            vec![(2, 1, formula("=SUM(A1:A2)"))],
        )]);
        let result = structural_inputs(
            &source,
            &StructuralOperation::DeleteRows {
                sheet: SheetId(1),
                start: RowIndex(0),
                count: 2,
            },
        )
        .expect("fully deleted range");
        assert_eq!(result.get(&address(1, 0, 1)), Some(&formula("=SUM(#REF!)")));
    }

    #[test]
    fn insert_column_moves_values_and_rewrites_formula_and_range() {
        let source = workbook(vec![sheet(
            1,
            "Sheet1",
            vec![
                (0, 0, CellInput::Number(10.0)),
                (0, 1, CellInput::Number(20.0)),
                (0, 2, formula("=A1+B1+SUM(A1:B1)")),
            ],
        )]);
        let result = structural_inputs(
            &source,
            &StructuralOperation::InsertColumns {
                sheet: SheetId(1),
                at: ColumnIndex(1),
                count: 1,
            },
        )
        .expect("insert column");
        assert_eq!(
            result.get(&address(1, 0, 2)),
            Some(&CellInput::Number(20.0))
        );
        assert_eq!(
            result.get(&address(1, 0, 3)),
            Some(&formula("=A1+C1+SUM(A1:C1)"))
        );
    }

    #[test]
    fn delete_column_preserves_native_broken_reference() {
        let source = workbook(vec![sheet(
            1,
            "Sheet1",
            vec![
                (0, 0, CellInput::Number(10.0)),
                (0, 1, CellInput::Number(20.0)),
                (0, 2, formula("=A1+B1")),
            ],
        )]);
        let result = structural_inputs(
            &source,
            &StructuralOperation::DeleteColumns {
                sheet: SheetId(1),
                start: ColumnIndex(1),
                count: 1,
            },
        )
        .expect("delete column");
        assert_eq!(result.get(&address(1, 0, 1)), Some(&formula("=A1+#REF!")));
    }

    #[test]
    fn literal_input_variants_survive_native_coordinate_moves_exactly() {
        let source = workbook(vec![sheet(
            1,
            "Sheet1",
            vec![
                (1, 0, CellInput::Percent(0.125)),
                (1, 1, CellInput::Date(46_000.0)),
                (1, 2, CellInput::Time(0.75)),
                (1, 3, CellInput::Text("=A1".into())),
            ],
        )]);
        let result = structural_inputs(
            &source,
            &StructuralOperation::InsertRows {
                sheet: SheetId(1),
                at: RowIndex(0),
                count: 2,
            },
        )
        .expect("insert rows");
        for (column, input) in [
            CellInput::Percent(0.125),
            CellInput::Date(46_000.0),
            CellInput::Time(0.75),
            CellInput::Text("=A1".into()),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(result.get(&address(1, 3, column as u32)), Some(&input));
        }
    }

    #[test]
    fn invalid_bounds_and_unsupported_formula_leave_snapshot_unchanged() {
        let source = workbook(vec![sheet(
            1,
            "Sheet1",
            vec![(MAX_ROWS - 1, 0, CellInput::Number(7.0))],
        )]);
        let before = source.clone();
        assert!(
            structural_inputs(
                &source,
                &StructuralOperation::InsertRows {
                    sheet: SheetId(1),
                    at: RowIndex(MAX_ROWS - 1),
                    count: 1,
                }
            )
            .is_err()
        );
        assert!(
            structural_inputs(
                &source,
                &StructuralOperation::DeleteColumns {
                    sheet: SheetId(1),
                    start: ColumnIndex(0),
                    count: 0,
                }
            )
            .is_err()
        );
        assert_eq!(source, before);

        let unsupported = workbook(vec![sheet(
            1,
            "Sheet1",
            vec![(0, 0, formula("=SEQUENCE(2)"))],
        )]);
        let before = unsupported.clone();
        assert!(
            structural_inputs(
                &unsupported,
                &StructuralOperation::InsertColumns {
                    sheet: SheetId(1),
                    at: ColumnIndex(0),
                    count: 1,
                }
            )
            .is_err()
        );
        assert_eq!(unsupported, before);
    }

    #[test]
    fn empty_sheet_can_receive_structural_insertion() {
        let source = workbook(vec![sheet(1, "Sheet1", Vec::new())]);
        assert!(
            structural_inputs(
                &source,
                &StructuralOperation::InsertRows {
                    sheet: SheetId(1),
                    at: RowIndex(0),
                    count: 1,
                }
            )
            .expect("insert into empty sheet")
            .is_empty()
        );
    }
}
