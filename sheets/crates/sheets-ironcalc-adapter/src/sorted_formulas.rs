//! Formula relocation for a bounded row permutation, through IronCalc's public
//! parsed-formula API. This is copy-relative relocation, NOT cut/paste: formulas
//! outside the sorted rectangle are intentionally left at their original addresses.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ironcalc::base::expressions::{
    parser::{Node, new_parser_english},
    token::Error as FormulaError,
    types::CellReferenceRC,
};
use sheets_core::{CellAddress, CellInput, CellRange, RowIndex, WorkbookSnapshot};

use crate::{AdapterError, model_from_snapshot, native_coordinate, native_sheet_index};

/// Returns relocated formulas keyed by destination address. Both relative row
/// references (including references to other local sheets) and mixed references
/// follow IronCalc's `extend_to`; absolute references stay absolute. No mutation
/// takes place here. Unsupported/invalid ASTs are rejected before canonical commit.
/// Conservative MVP: known array constructors are rejected even inside scalar
/// aggregates, unmoved rows, or unmapped headers inside the rectangle. No extra
/// engine evaluation is used; formulas outside the rectangle are not validated.
/// Relocation must not introduce new parsed REF errors; deliberate existing
/// error literals remain valid formula inputs.
pub fn translate_sorted_formulas(
    snapshot: &WorkbookSnapshot,
    range: &CellRange,
    row_moves: &[(RowIndex, RowIndex)],
) -> Result<BTreeMap<CellAddress, String>, AdapterError> {
    range
        .validate()
        .map_err(|e| AdapterError::Snapshot(e.to_string()))?;
    if range.rows().saturating_mul(range.cols()) > 10_000 {
        return Err(AdapterError::Snapshot(
            "sort formula rectangle exceeds 10000 cells".into(),
        ));
    }
    let mut moves = BTreeMap::new();
    let mut targets = BTreeSet::new();
    for &(source, target) in row_moves {
        if source < range.start.row
            || source > range.end.row
            || target < range.start.row
            || target > range.end.row
            || moves.insert(source, target).is_some()
            || !targets.insert(target)
        {
            return Err(AdapterError::Snapshot(
                "invalid sort row permutation".into(),
            ));
        }
    }
    if moves.keys().copied().collect::<BTreeSet<_>>() != targets {
        return Err(AdapterError::Snapshot(
            "sort row mapping is not a permutation".into(),
        ));
    }
    let sheet_index = snapshot
        .sheets
        .iter()
        .position(|s| s.id == range.sheet)
        .ok_or_else(|| AdapterError::Snapshot("sort sheet is missing".into()))?;
    let sheet = &snapshot.sheets[sheet_index];
    let mut formula_cells: Vec<_> = sheet
        .cells
        .iter()
        .filter(|cell| range.contains(&cell.address) && matches!(cell.input, CellInput::Formula(_)))
        .collect();
    if formula_cells.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut parser = new_parser_english(
        snapshot.sheets.iter().map(|s| s.name.clone()).collect(),
        Vec::new(),
        HashMap::new(),
    );
    let mut source_ref_errors = BTreeMap::new();
    // Validate every in-rectangle AST before converting any workbook data to
    // an engine model or mapping destinations. An unmoved/unmapped array anchor
    // (including a header) can still spill into moved rows.
    for cell in &formula_cells {
        let CellInput::Formula(body) = &cell.input else {
            unreachable!()
        };
        let source_row = native_coordinate(cell.address.row.0, "row")?;
        let column = native_coordinate(cell.address.column.0, "column")?;
        let source_context = CellReferenceRC {
            sheet: sheet.name.clone(),
            row: source_row,
            column,
        };
        let ast = parser.parse(body.trim_start_matches('='), &source_context);
        let ref_errors = validate_relocation(&ast, 0).map_err(|reason| {
            AdapterError::Snapshot(format!(
                "cannot sort formula at row {}, column {}: {reason}",
                source_row, column
            ))
        })?;
        source_ref_errors.insert(cell.address, ref_errors);
    }
    formula_cells.retain(|cell| {
        moves
            .get(&cell.address.row)
            .is_some_and(|row| *row != cell.address.row)
    });
    if formula_cells.is_empty() {
        return Ok(BTreeMap::new());
    }
    let model = model_from_snapshot(snapshot)?;
    let native_sheet = native_sheet_index(sheet_index)?;
    let mut translated = BTreeMap::new();
    for cell in formula_cells {
        let destination_row = moves[&cell.address.row];
        let source_row = native_coordinate(cell.address.row.0, "row")?;
        let column = native_coordinate(cell.address.column.0, "column")?;
        let target_row = native_coordinate(destination_row.0, "row")?;
        let formula = model
            .extend_to(native_sheet, source_row, column, target_row, column)
            .map_err(AdapterError::Engine)?;
        let target_context = CellReferenceRC {
            sheet: sheet.name.clone(),
            row: target_row,
            column,
        };
        let target_ref_errors = validate_relocation(
            &parser.parse(formula.trim_start_matches('='), &target_context),
            0,
        )
        .map_err(|reason| {
            AdapterError::Snapshot(format!("invalid relocated sort formula: {reason}"))
        })?;
        if target_ref_errors > source_ref_errors[&cell.address] {
            return Err(AdapterError::Snapshot(format!(
                "sort relocation would introduce #REF! at row {target_row}, column {column}"
            )));
        }
        translated.insert(
            CellAddress::new(range.sheet, destination_row, cell.address.column),
            formula,
        );
    }
    Ok(translated)
}

/// Validates the AST and counts explicit REF error nodes in the same walk.
fn validate_relocation(node: &Node, depth: usize) -> Result<usize, &'static str> {
    if depth > 256 {
        return Err("formula nesting exceeds relocation budget");
    }
    match node {
        Node::ParseErrorKind { .. } => Err(
            "unsupported or invalid formula syntax (including external workbook/structured references)",
        ),
        Node::WrongReferenceKind { .. } | Node::WrongRangeKind { .. } => {
            Err("unresolved sheet reference")
        }
        Node::DefinedNameKind(_) | Node::TableNameKind(_) | Node::NamedVariableKind { .. } => {
            Err("named/structured references are outside canonical sort support")
        }
        Node::SpillRangeOperator { .. }
        | Node::ArrayKind(_)
        | Node::LambdaDefKind { .. }
        | Node::LambdaCallKind { .. } => {
            Err("array/spill/lambda formula relocation is not supported")
        }
        Node::OpRangeKind { left, right }
        | Node::OpConcatenateKind { left, right }
        | Node::OpSumKind { left, right, .. }
        | Node::OpProductKind { left, right, .. }
        | Node::OpPowerKind { left, right }
        | Node::CompareKind { left, right, .. } => {
            let left_errors = validate_relocation(left, depth + 1)?;
            Ok(left_errors.saturating_add(validate_relocation(right, depth + 1)?))
        }
        Node::UnaryKind { right, .. } => validate_relocation(right, depth + 1),
        Node::ImplicitIntersection { child, .. } => validate_relocation(child, depth + 1),
        Node::FunctionKind { kind, args } => {
            if is_dynamic_array_constructor_name(&kind.to_xlsx_string()) {
                return Err("dynamic-array constructor is outside canonical sort support");
            }
            args.iter().try_fold(0usize, |count, node| {
                validate_relocation(node, depth + 1).map(|errors| count.saturating_add(errors))
            })
        }
        Node::NamedFunctionKind { args, .. } => args.iter().try_fold(0usize, |count, node| {
            validate_relocation(node, depth + 1).map(|errors| count.saturating_add(errors))
        }),
        Node::ErrorKind(error) => Ok(usize::from(*error == FormulaError::REF)),
        Node::BooleanKind(_)
        | Node::NumberKind(_)
        | Node::StringKind(_)
        | Node::ReferenceKind { .. }
        | Node::RangeKind { .. }
        | Node::EmptyArgKind => Ok(0),
    }
}

/// Exact public AST function names, never a search through formula text. The
/// list includes legacy array producers and conservatively rejects nested uses
/// even when an outer aggregate, IF branch, or @ would otherwise return a scalar.
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
    use sheets_core::{Change, ChangeSet, ColumnIndex, Workbook};

    fn snapshot_with_formula(body: &str) -> (WorkbookSnapshot, CellRange) {
        let workbook = Workbook::new();
        let sheet = workbook.active_sheet();
        let start = CellAddress::new(sheet, RowIndex(1), ColumnIndex(0));
        let end = CellAddress::new(sheet, RowIndex(2), ColumnIndex(3));
        let mut changes = ChangeSet::new();
        changes.push(Change::SetCellInput {
            sheet,
            address: CellAddress::new(sheet, RowIndex(1), ColumnIndex(3)),
            input: CellInput::formula(body).unwrap(),
        });
        (
            workbook
                .prepare(changes)
                .unwrap()
                .engine_request()
                .workbook()
                .clone(),
            CellRange::new(start, end).unwrap(),
        )
    }
    fn moved(body: &str) -> Result<String, AdapterError> {
        let (snapshot, range) = snapshot_with_formula(body);
        let result = translate_sorted_formulas(
            &snapshot,
            &range,
            &[(RowIndex(1), RowIndex(2)), (RowIndex(2), RowIndex(1))],
        )?;
        Ok(result[&range.end].clone())
    }
    fn moved_up_from_b2(body: &str) -> Result<String, AdapterError> {
        let (mut snapshot, _) = snapshot_with_formula(body);
        let sheet = snapshot.sheets[0].id;
        snapshot.sheets[0].cells[0].address.column = ColumnIndex(1);
        let target = CellAddress::new(sheet, RowIndex(0), ColumnIndex(1));
        let source = CellAddress::new(sheet, RowIndex(1), ColumnIndex(1));
        let range = CellRange::new(target, source).unwrap();
        let translated = translate_sorted_formulas(
            &snapshot,
            &range,
            &[(RowIndex(0), RowIndex(1)), (RowIndex(1), RowIndex(0))],
        )?;
        Ok(translated[&target].clone())
    }
    #[test]
    fn upward_formula_relocation_rejects_new_ref_errors() {
        for body in ["=A1", "=$A1", "=#REF!+A1", "=IFERROR(A1,\"#REF!\")"] {
            let result = moved_up_from_b2(body);
            assert!(
                matches!(&result, Err(AdapterError::Snapshot(reason))
                    if reason.contains("introduce #REF!")),
                "{body}: expected introduced REF rejection for B2 -> B1, got {result:?}"
            );
        }
    }
    #[test]
    fn upward_formula_relocation_preserves_absolute_rows_and_existing_ref_errors() {
        for body in ["=A$1", "=$A$1", "=#REF!+A$1", "=IFERROR(#REF!,A$1)"] {
            assert_eq!(moved_up_from_b2(body).unwrap(), body);
        }
    }
    #[test]
    fn row_formula_relocation_uses_engine_relative_and_mixed_references() {
        assert_eq!(moved("=B2*C2").unwrap(), "=B3*C3");
        assert_eq!(moved("=B2*$C$2+$B2+C$2").unwrap(), "=B3*$C$2+$B3+C$2");
        assert_eq!(moved("=SUM(B2:C2)").unwrap(), "=SUM(B3:C3)");
    }
    #[test]
    fn relocation_does_not_rewrite_reference_looking_string_literals() {
        assert_eq!(
            moved("=IF(B2=1,\"A1 [Book.xlsx] Привет\",\"B2\")").unwrap(),
            "=IF(B3=1,\"A1 [Book.xlsx] Привет\",\"B2\")"
        );
    }
    #[test]
    fn relocation_preserves_quoted_cyrillic_local_sheet_references() {
        let (mut snapshot, range) = snapshot_with_formula("='Данные 2'!B2+$B$2");
        snapshot.sheets.push(sheets_core::SheetSnapshot {
            id: sheets_core::SheetId(range.sheet.0 + 1),
            name: "Данные 2".into(),
            ..Default::default()
        });
        let result = translate_sorted_formulas(
            &snapshot,
            &range,
            &[(RowIndex(1), RowIndex(2)), (RowIndex(2), RowIndex(1))],
        )
        .unwrap();
        assert_eq!(result[&range.end], "='Данные 2'!B3+$B$2");
    }
    #[test]
    fn unsupported_reference_is_a_controlled_error_not_replacement_text() {
        assert!(moved("='[Other.xlsx]Data'!B2").is_err());
        assert!(moved("=Missing!B2").is_err());
        assert!(moved("=B2+").is_err());
        assert!(moved("=SUM(Table1[Qty])").is_err());
    }
    #[test]
    fn dynamic_array_constructors_are_rejected_by_ast_kind() {
        for body in [
            "=SEQUENCE(2)",
            "=sequence(2)",
            "=_xlfn.SEQUENCE(2)",
            "=RANDARRAY(2)",
            "=FILTER(B2:C3,B2:B3>0)",
            "=SORT(B2:C3)",
            "=_xlfn._xlws.SORT(B2:C3)",
            "=SORTBY(B2:C3,B2:B3,1)",
            "=UNIQUE(B2:C3)",
            "=TRANSPOSE(B2:C2)",
            "=TAKE(B2:C3,1)",
            "=DROP(B2:C3,1)",
            "=EXPAND(B2:C2,2)",
            "=CHOOSECOLS(B2:C3,1)",
            "=CHOOSEROWS(B2:C3,1)",
            "=HSTACK(B2,C2)",
            "=VSTACK(B2,C2)",
            "=TOCOL(B2:C3)",
            "=TOROW(B2:C3)",
            "=WRAPCOLS(B2:C2,1)",
            "=WRAPROWS(B2:C2,1)",
            "=TEXTSPLIT(\"a,b\",\",\")",
            "=TRIMRANGE(B2:C3)",
            "=BYCOL(B2:C3,LAMBDA(x,SUM(x)))",
            "=BYROW(B2:C3,LAMBDA(x,SUM(x)))",
            "=MAP(B2:B3,LAMBDA(x,x))",
            "=REDUCE(0,B2:B3,LAMBDA(a,x,a+x))",
            "=SCAN(0,B2:B3,LAMBDA(a,x,a+x))",
            "=MAKEARRAY(2,1,LAMBDA(r,c,r+c))",
            "=FREQUENCY(B2:B3,C2:C3)",
            "=LINEST(B2:B3,C2:C3)",
            "=LOGEST(B2:B3,C2:C3)",
            "=TREND(B2:B3,C2:C3)",
            "=GROWTH(B2:B3,C2:C3)",
            "=MINVERSE(B2:C3)",
            "=MMULT(B2:C3,B2:C3)",
            "=MUNIT(2)",
            "=MODE.MULT(B2:B3)",
            "=REGEXEXTRACT(\"ab\",\"a\",1)",
        ] {
            let result = moved(body);
            assert!(
                matches!(&result, Err(AdapterError::Snapshot(reason))
                    if reason.contains("dynamic-array constructor")),
                "{body}: expected AST constructor rejection, got {result:?}"
            );
        }
    }
    #[test]
    fn dynamic_array_nested_constructors_are_conservatively_rejected() {
        for body in [
            "=SUM(SEQUENCE(2))",
            "=IF(FALSE,SEQUENCE(2),7)",
            "=@SEQUENCE(2)",
            "=1+SUM(TRANSPOSE(B2:C2))",
        ] {
            let result = moved(body);
            assert!(
                matches!(&result, Err(AdapterError::Snapshot(reason))
                    if reason.contains("dynamic-array constructor")),
                "{body}: expected nested constructor rejection, got {result:?}"
            );
        }
    }
    #[test]
    fn dynamic_array_preflight_runs_before_model_conversion() {
        let (mut snapshot, range) = snapshot_with_formula("=SEQUENCE(2)");
        // This unrelated coordinate would fail model conversion if it were reached.
        snapshot.sheets[0].cells.push(sheets_core::CellSnapshot {
            address: CellAddress::new(range.sheet, RowIndex(u32::MAX), ColumnIndex(0)),
            input: CellInput::Number(1.0),
            format: None,
        });
        let before = snapshot.clone();
        let result = translate_sorted_formulas(
            &snapshot,
            &range,
            &[(RowIndex(1), RowIndex(2)), (RowIndex(2), RowIndex(1))],
        );
        assert!(
            matches!(&result, Err(AdapterError::Snapshot(reason))
                if reason.contains("dynamic-array constructor")),
            "expected preflight rejection before model conversion, got {result:?}"
        );
        assert_eq!(snapshot, before);
    }
    #[test]
    fn dynamic_array_stationary_formula_in_permuted_rows_is_rejected() {
        let (snapshot, mut range) = snapshot_with_formula("=SEQUENCE(2)");
        range.end.row = RowIndex(3);
        let result = translate_sorted_formulas(
            &snapshot,
            &range,
            &[
                (RowIndex(1), RowIndex(1)),
                (RowIndex(2), RowIndex(3)),
                (RowIndex(3), RowIndex(2)),
            ],
        );
        assert!(
            matches!(&result, Err(AdapterError::Snapshot(reason))
                if reason.contains("dynamic-array constructor")),
            "expected stationary constructor rejection, got {result:?}"
        );
    }
    #[test]
    fn dynamic_array_constructor_text_in_literals_is_not_rejected() {
        assert_eq!(
            moved("=SUM(B2:C2)+LEN(\"SEQUENCE(2) _xlfn.FILTER(A1)\")").unwrap(),
            "=SUM(B3:C3)+LEN(\"SEQUENCE(2) _xlfn.FILTER(A1)\")"
        );
    }
    #[test]
    fn dynamic_array_outside_rectangle_does_not_reject_scalar_relocation() {
        let (mut snapshot, range) = snapshot_with_formula("=B2");
        snapshot.sheets[0].cells.push(sheets_core::CellSnapshot {
            address: CellAddress::new(range.sheet, RowIndex(1), ColumnIndex(4)),
            input: CellInput::formula("=SEQUENCE(2)").unwrap(),
            format: None,
        });
        let before = snapshot.clone();
        let translated = translate_sorted_formulas(
            &snapshot,
            &range,
            &[(RowIndex(1), RowIndex(2)), (RowIndex(2), RowIndex(1))],
        )
        .unwrap();
        assert_eq!(translated[&range.end], "=B3");
        assert_eq!(snapshot, before);
    }
    #[test]
    fn dynamic_array_unmapped_header_anchor_is_rejected_before_relocation() {
        let (mut snapshot, mut range) = snapshot_with_formula("=B2");
        range.start.row = RowIndex(0);
        snapshot.sheets[0].cells.push(sheets_core::CellSnapshot {
            address: CellAddress::new(range.sheet, RowIndex(0), ColumnIndex(3)),
            input: CellInput::formula("=SEQUENCE(3)").unwrap(),
            format: None,
        });
        let before = snapshot.clone();
        let result = translate_sorted_formulas(
            &snapshot,
            &range,
            &[(RowIndex(1), RowIndex(2)), (RowIndex(2), RowIndex(1))],
        );
        assert!(
            matches!(&result, Err(AdapterError::Snapshot(reason))
                if reason.contains("dynamic-array constructor")),
            "expected rejection of an unmapped in-rectangle anchor, got {result:?}"
        );
        assert_eq!(snapshot, before);
    }
    #[test]
    fn dynamic_array_preflight_preserves_scalar_header_and_outside_invalid_formula() {
        let (mut snapshot, mut range) = snapshot_with_formula("=B2");
        range.start.row = RowIndex(0);
        snapshot.sheets[0].cells.extend([
            sheets_core::CellSnapshot {
                address: CellAddress::new(range.sheet, RowIndex(0), ColumnIndex(3)),
                input: CellInput::formula("=SUM(B1:C1)").unwrap(),
                format: None,
            },
            sheets_core::CellSnapshot {
                address: CellAddress::new(range.sheet, RowIndex(0), ColumnIndex(4)),
                input: CellInput::formula("='[Other.xlsx]Data'!B2").unwrap(),
                format: None,
            },
        ]);
        let before = snapshot.clone();
        let translated = translate_sorted_formulas(
            &snapshot,
            &range,
            &[(RowIndex(1), RowIndex(2)), (RowIndex(2), RowIndex(1))],
        )
        .unwrap();
        assert_eq!(translated.len(), 1);
        assert_eq!(translated[&range.end], "=B3");
        assert_eq!(snapshot, before);
    }
    #[test]
    fn formulas_outside_rectangle_are_not_checked_or_rewritten() {
        let (snapshot, mut range) = snapshot_with_formula("='[Other.xlsx]Data'!B2");
        range.end.column = ColumnIndex(2);
        assert!(
            translate_sorted_formulas(
                &snapshot,
                &range,
                &[(RowIndex(1), RowIndex(2)), (RowIndex(2), RowIndex(1))]
            )
            .unwrap()
            .is_empty()
        );
    }
    #[test]
    fn malformed_permutation_is_rejected_before_engine() {
        let (snapshot, range) = snapshot_with_formula("=B2");
        assert!(
            translate_sorted_formulas(&snapshot, &range, &[(RowIndex(1), RowIndex(2))]).is_err()
        );
        assert!(
            translate_sorted_formulas(
                &snapshot,
                &range,
                &[(RowIndex(1), RowIndex(2)), (RowIndex(2), RowIndex(2))]
            )
            .is_err()
        );
    }
}
