use crate::*;
use sheets_core::{
    AutoFilter, CellAddress, CellError, CellFormat, CellInput, CellRange, CellSnapshot, CellValue,
    Color, ColumnIndex, FilterCriterion, Font, RowIndex, SheetId, SheetSnapshot, SpreadsheetEngine,
    Workbook,
};
use std::collections::BTreeMap;

fn address(sheet: SheetId, row: u32, column: u32) -> CellAddress {
    CellAddress::new(sheet, RowIndex(row), ColumnIndex(column))
}

fn range(
    sheet: SheetId,
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

fn put<E: SpreadsheetEngine>(
    api: &mut SpreadsheetApi<E>,
    sheet: SheetId,
    row: u32,
    column: u32,
    input: CellInput,
) {
    api.set_cell_input(sheet, address(sheet, row, column), input)
        .unwrap();
}

fn put_text<E: SpreadsheetEngine>(
    api: &mut SpreadsheetApi<E>,
    sheet: SheetId,
    row: u32,
    column: u32,
    text: &str,
) {
    put(api, sheet, row, column, CellInput::Text(text.to_owned()));
}

fn put_formula<E: SpreadsheetEngine>(
    api: &mut SpreadsheetApi<E>,
    sheet: SheetId,
    row: u32,
    column: u32,
    formula: &str,
) {
    put(
        api,
        sheet,
        row,
        column,
        CellInput::formula(formula).unwrap(),
    );
}

fn state<E: SpreadsheetEngine>(api: &SpreadsheetApi<E>, sheet: SheetId) -> SheetSnapshot {
    api.document()
        .snapshot()
        .sheets
        .into_iter()
        .find(|candidate| candidate.id == sheet)
        .unwrap()
}

fn assert_number<E: SpreadsheetEngine>(
    api: &SpreadsheetApi<E>,
    sheet: SheetId,
    row: u32,
    column: u32,
    expected: f64,
) {
    match api
        .document()
        .cell_value(sheet, address(sheet, row, column))
    {
        Some(CellValue::Number(value)) => assert_eq!(*value, expected),
        other => panic!("expected number {expected} at ({row}, {column}), got {other:?}"),
    }
}

fn assert_structural_rejection(error: &ApiError) {
    assert!(matches!(
        error.kind(),
        ApiErrorKind::Unsupported(ApiUnsupportedError::StructuralEdit(_))
    ));
}

#[test]
fn insert_rows_rebases_relative_absolute_and_mixed_references_and_is_reversible() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(2.0));
    put(&mut api, sheet, 2, 0, CellInput::Number(5.0));
    put_formula(&mut api, sheet, 3, 1, "=$A1+A$3+$A$3+$A3");
    assert_number(&api, sheet, 3, 1, 17.0);
    let before = state(&api, sheet);
    let history = api.history_stats();

    api.insert_rows(sheet, RowIndex(2), 1).unwrap();

    assert_eq!(
        api.document()
            .cell_input(sheet, address(sheet, 4, 1))
            .and_then(CellInput::as_formula),
        Some("$A1+A$4+$A$4+$A4")
    );
    assert_number(&api, sheet, 4, 1, 17.0);
    let after = state(&api, sheet);
    assert_eq!(api.history_stats().undo_entries, history.undo_entries + 1);

    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    assert_number(&api, sheet, 3, 1, 17.0);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);
    assert_number(&api, sheet, 4, 1, 17.0);
}

#[test]
fn insert_columns_rebases_relative_absolute_and_mixed_references() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(2.0));
    put(&mut api, sheet, 0, 2, CellInput::Number(5.0));
    put_formula(&mut api, sheet, 3, 3, "=A1+C1+$C1+C$1+$C$1");
    assert_number(&api, sheet, 3, 3, 22.0);
    let before = state(&api, sheet);

    api.insert_columns(sheet, ColumnIndex(2), 1).unwrap();

    assert_eq!(
        api.document()
            .cell_input(sheet, address(sheet, 3, 4))
            .and_then(CellInput::as_formula),
        Some("A1+D1+$D1+D$1+$D$1")
    );
    assert_number(&api, sheet, 3, 4, 22.0);
    let after = state(&api, sheet);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);
    assert_number(&api, sheet, 3, 4, 22.0);
}

#[test]
fn count_two_axis_inserts_move_mixed_formula_custom_font_and_keep_default_axes_implicit() {
    let config = DocumentConfig::new(100, 32, 22, 88);
    let mut api = SpreadsheetApi::with_config(IroncalcAdapter::new(), config).unwrap();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(2.0));
    put(&mut api, sheet, 0, 2, CellInput::Number(5.0));
    put_formula(&mut api, sheet, 3, 3, "=A1+C1+$C1+C$1+$C$1");
    let style = CellFormat {
        font: Font {
            family: "Structural Fixture Sans".into(),
            size: 14,
            bold: true,
            italic: true,
            underline: false,
        },
        background: Some(Color::new(30, 60, 90, 255)),
        ..CellFormat::default()
    };
    api.set_cell_format(
        sheet,
        address(sheet, 3, 3),
        style.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    api.set_row_dimension(sheet, RowIndex(3), 41.0).unwrap();
    api.set_column_dimension(sheet, ColumnIndex(3), 123.0)
        .unwrap();
    assert_number(&api, sheet, 3, 3, 22.0);
    let before = state(&api, sheet);

    api.insert_rows(sheet, RowIndex(2), 2).unwrap();
    let row_inserted = state(&api, sheet);
    api.insert_columns(sheet, ColumnIndex(2), 2).unwrap();

    let after = state(&api, sheet);
    assert_eq!(
        api.document()
            .cell_input(sheet, address(sheet, 5, 5))
            .and_then(CellInput::as_formula),
        Some("A1+E1+$E1+E$1+$E$1")
    );
    assert_number(&api, sheet, 5, 5, 22.0);
    assert_eq!(
        api.document().cell_format(sheet, address(sheet, 5, 5)),
        Some(&style)
    );
    assert_eq!(after.row_dimensions[0].row, RowIndex(5));
    assert_eq!(after.row_dimensions[0].height, 41.0);
    assert_eq!(after.column_dimensions[0].column, ColumnIndex(5));
    assert_eq!(after.column_dimensions[0].width, 123.0);
    assert!(
        !after
            .row_dimensions
            .iter()
            .any(|dimension| matches!(dimension.row, RowIndex(2 | 3)))
    );
    assert!(
        !after
            .column_dimensions
            .iter()
            .any(|dimension| matches!(dimension.column, ColumnIndex(2 | 3)))
    );
    assert!(after.cells.iter().all(|cell| {
        !matches!(cell.address.row, RowIndex(2 | 3))
            && !matches!(cell.address.column, ColumnIndex(2 | 3))
    }));
    assert_eq!(api.config().default_row_height, 22);
    assert_eq!(api.config().default_column_width, 88);

    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), row_inserted);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), row_inserted);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);
}

#[test]
fn insert_inside_sum_range_expands_formula_range() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    for (row, value) in [(0, 2.0), (1, 3.0), (2, 4.0)] {
        put(&mut api, sheet, row, 0, CellInput::Number(value));
    }
    put_formula(&mut api, sheet, 0, 1, "=SUM(A1:A3)");
    let before = state(&api, sheet);
    assert_number(&api, sheet, 0, 1, 9.0);

    api.insert_rows(sheet, RowIndex(1), 1).unwrap();

    let formula = api
        .document()
        .cell_input(sheet, address(sheet, 0, 1))
        .and_then(CellInput::as_formula)
        .unwrap();
    assert!(formula.contains("A1:A4"), "formula was {formula}");
    assert_number(&api, sheet, 0, 1, 9.0);
    let after = state(&api, sheet);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);
}

#[test]
fn delete_interior_row_shrinks_sum_range_and_restores_exact_inputs() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    for (row, value) in [(0, 2.0), (1, 3.0), (2, 4.0), (3, 8.0)] {
        put(&mut api, sheet, row, 0, CellInput::Number(value));
    }
    put_formula(&mut api, sheet, 0, 1, "=SUM(A1:A3)");
    let before = state(&api, sheet);
    assert_number(&api, sheet, 0, 1, 9.0);

    api.delete_rows(sheet, RowIndex(1), 1).unwrap();

    let formula = api
        .document()
        .cell_input(sheet, address(sheet, 0, 1))
        .and_then(CellInput::as_formula)
        .unwrap();
    assert!(formula.contains("A1:A2"), "formula was {formula}");
    assert!(!formula.contains("A3"), "formula was {formula}");
    assert_number(&api, sheet, 0, 1, 6.0);
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 2, 0)),
        Some(&CellInput::Number(8.0))
    );
    let after = state(&api, sheet);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    assert_number(&api, sheet, 0, 1, 9.0);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);
    assert_number(&api, sheet, 0, 1, 6.0);
}

#[test]
fn deleting_sum_endpoint_shrinks_range_and_is_reversible() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    for (row, value) in [(0, 2.0), (1, 3.0), (2, 4.0), (3, 8.0)] {
        put(&mut api, sheet, row, 0, CellInput::Number(value));
    }
    put_formula(&mut api, sheet, 0, 1, "=SUM(A1:A3)");
    assert_number(&api, sheet, 0, 1, 9.0);
    let before = state(&api, sheet);

    api.delete_rows(sheet, RowIndex(2), 1).unwrap();

    let formula = api
        .document()
        .cell_input(sheet, address(sheet, 0, 1))
        .and_then(CellInput::as_formula)
        .unwrap();
    assert!(formula.contains("A1:A2"), "formula was {formula}");
    assert!(!formula.contains("A3"), "formula was {formula}");
    assert_number(&api, sheet, 0, 1, 5.0);
    let after = state(&api, sheet);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    assert_number(&api, sheet, 0, 1, 9.0);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);
    assert_number(&api, sheet, 0, 1, 5.0);
}

#[test]
fn deleting_the_full_sum_range_produces_ref_and_is_reversible() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 1, 0, CellInput::Number(3.0));
    put(&mut api, sheet, 2, 0, CellInput::Number(4.0));
    put_formula(&mut api, sheet, 0, 1, "=SUM(A2:A3)");
    assert_number(&api, sheet, 0, 1, 7.0);
    let before = state(&api, sheet);

    api.delete_rows(sheet, RowIndex(1), 2).unwrap();

    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 0, 1)),
        Some(&CellValue::Error(CellError::Ref))
    );
    let after = state(&api, sheet);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    assert_number(&api, sheet, 0, 1, 7.0);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);
    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 0, 1)),
        Some(&CellValue::Error(CellError::Ref))
    );
}

#[test]
fn delete_column_containing_formula_reference_produces_ref_and_is_reversible() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 0, 2, CellInput::Number(11.0));
    put_formula(&mut api, sheet, 0, 0, "=C1");
    assert_number(&api, sheet, 0, 0, 11.0);
    let before = state(&api, sheet);

    api.delete_columns(sheet, ColumnIndex(2), 1).unwrap();

    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 0, 0)),
        Some(&CellValue::Error(CellError::Ref))
    );
    let after = state(&api, sheet);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    assert_number(&api, sheet, 0, 0, 11.0);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);
    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 0, 0)),
        Some(&CellValue::Error(CellError::Ref))
    );
}

#[test]
fn deleting_column_sum_endpoint_shrinks_range_and_is_reversible() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    for (column, value) in [(0, 2.0), (1, 3.0), (2, 4.0), (3, 8.0)] {
        put(&mut api, sheet, 0, column, CellInput::Number(value));
    }
    put_formula(&mut api, sheet, 0, 4, "=SUM(A1:C1)");
    assert_number(&api, sheet, 0, 4, 9.0);
    let before = state(&api, sheet);

    api.delete_columns(sheet, ColumnIndex(2), 1).unwrap();

    let formula = api
        .document()
        .cell_input(sheet, address(sheet, 0, 3))
        .and_then(CellInput::as_formula)
        .unwrap();
    assert!(formula.contains("A1:B1"), "formula was {formula}");
    assert!(!formula.contains("C1"), "formula was {formula}");
    assert_number(&api, sheet, 0, 3, 5.0);
    let after = state(&api, sheet);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    assert_number(&api, sheet, 0, 4, 9.0);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);
    assert_number(&api, sheet, 0, 3, 5.0);
}

#[test]
fn cross_sheet_formula_references_follow_row_insert_and_undo_redo() {
    let mut api = SpreadsheetApi::production();
    let source = api.document().active_sheet();
    api.add_sheet(Some("Other".into())).unwrap();
    let formula_sheet = api.document().active_sheet();
    put(&mut api, source, 0, 0, CellInput::Number(9.0));
    put(&mut api, source, 0, 1, CellInput::Number(-4.0));
    put_formula(
        &mut api,
        formula_sheet,
        0,
        1,
        "=Sheet1!A1+Sheet1!$B1+Sheet1!B$1+Sheet1!$B$1",
    );
    assert_number(&api, formula_sheet, 0, 1, -3.0);
    let before = api.document().snapshot().sheets;

    api.insert_rows(source, RowIndex(0), 1).unwrap();

    let formula = api
        .document()
        .cell_input(formula_sheet, address(formula_sheet, 0, 1))
        .and_then(CellInput::as_formula)
        .unwrap();
    assert_eq!(formula, "Sheet1!A2+Sheet1!$B2+Sheet1!B$2+Sheet1!$B$2");
    assert_number(&api, formula_sheet, 0, 1, -3.0);
    let after = api.document().snapshot().sheets;
    api.undo().unwrap();
    assert_eq!(api.document().snapshot().sheets, before);
    assert_number(&api, formula_sheet, 0, 1, -3.0);
    api.redo().unwrap();
    assert_eq!(api.document().snapshot().sheets, after);
    assert_number(&api, formula_sheet, 0, 1, -3.0);
}

#[test]
fn structural_edits_shift_raw_formats_dimensions_and_manual_hidden_metadata() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    let original_format = CellFormat {
        background: Some(Color::new(20, 40, 60, 255)),
        ..CellFormat::default()
    };
    put(&mut api, sheet, 2, 1, CellInput::Text("row payload".into()));
    api.set_cell_format(
        sheet,
        address(sheet, 2, 1),
        original_format.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    api.set_row_dimension(sheet, RowIndex(2), 42.0).unwrap();
    api.set_manual_hidden_rows(sheet, vec![RowIndex(2)])
        .unwrap();
    let row_before = state(&api, sheet);

    api.insert_rows(sheet, RowIndex(2), 1).unwrap();

    let row_after = state(&api, sheet);
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 3, 1)),
        Some(&CellInput::Text("row payload".into()))
    );
    assert_eq!(
        api.document().cell_format(sheet, address(sheet, 3, 1)),
        Some(&original_format)
    );
    assert_eq!(row_after.row_dimensions[0].row, RowIndex(3));
    assert_eq!(row_after.row_dimensions[0].height, 42.0);
    assert_eq!(row_after.manual_hidden_rows, [RowIndex(3)]);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), row_before);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), row_after);

    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(
        &mut api,
        sheet,
        1,
        2,
        CellInput::Text("column payload".into()),
    );
    api.set_cell_format(
        sheet,
        address(sheet, 1, 2),
        original_format.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    api.set_column_dimension(sheet, ColumnIndex(2), 128.0)
        .unwrap();
    api.set_manual_hidden_columns(sheet, vec![ColumnIndex(2)])
        .unwrap();
    let column_before = state(&api, sheet);

    api.insert_columns(sheet, ColumnIndex(2), 1).unwrap();

    let column_after = state(&api, sheet);
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 1, 3)),
        Some(&CellInput::Text("column payload".into()))
    );
    assert_eq!(
        api.document().cell_format(sheet, address(sheet, 1, 3)),
        Some(&original_format)
    );
    assert_eq!(column_after.column_dimensions[0].column, ColumnIndex(3));
    assert_eq!(column_after.column_dimensions[0].width, 128.0);
    assert_eq!(column_after.manual_hidden_columns, [ColumnIndex(3)]);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), column_before);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), column_after);
}

#[test]
fn filters_follow_row_and_column_shifts_and_disappear_when_fully_deleted() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 2, 1, "Status");
    put(&mut api, sheet, 2, 2, CellInput::Text("Count".into()));
    put_text(&mut api, sheet, 3, 1, "keep");
    put(&mut api, sheet, 3, 2, CellInput::Number(1.0));
    put_text(&mut api, sheet, 4, 1, "drop");
    put(&mut api, sheet, 4, 2, CellInput::Number(1.0));
    let mut criteria = BTreeMap::new();
    criteria.insert(ColumnIndex(1), FilterCriterion::Equals("keep".into()));
    criteria.insert(ColumnIndex(2), FilterCriterion::GreaterThan(0.0));
    api.set_auto_filter(
        sheet,
        AutoFilter {
            range: range(sheet, 2, 0, 4, 2),
            has_header: true,
            criteria,
        },
    )
    .unwrap();
    assert_eq!(api.filter_hidden_rows(sheet), [RowIndex(4)]);

    api.insert_rows(sheet, RowIndex(0), 1).unwrap();
    assert_eq!(api.auto_filter(sheet).unwrap().range.start.row, RowIndex(3));
    assert_eq!(api.auto_filter(sheet).unwrap().range.end.row, RowIndex(5));
    assert_eq!(api.filter_hidden_rows(sheet), [RowIndex(5)]);
    api.insert_rows(sheet, RowIndex(4), 1).unwrap();
    assert_eq!(api.auto_filter(sheet).unwrap().range.end.row, RowIndex(6));
    assert_eq!(api.filter_hidden_rows(sheet), [RowIndex(4), RowIndex(6)]);
    api.insert_rows(sheet, RowIndex(7), 1).unwrap();
    assert_eq!(api.auto_filter(sheet).unwrap().range.end.row, RowIndex(6));
    assert_eq!(api.filter_hidden_rows(sheet), [RowIndex(4), RowIndex(6)]);

    api.insert_columns(sheet, ColumnIndex(1), 1).unwrap();
    let filter = api.auto_filter(sheet).unwrap();
    assert_eq!(filter.range.start.column, ColumnIndex(0));
    assert_eq!(filter.range.end.column, ColumnIndex(3));
    assert!(filter.criteria.contains_key(&ColumnIndex(2)));
    assert!(filter.criteria.contains_key(&ColumnIndex(3)));
    assert_eq!(api.filter_hidden_rows(sheet), [RowIndex(4), RowIndex(6)]);

    api.delete_columns(sheet, ColumnIndex(2), 1).unwrap();
    let filter = api.auto_filter(sheet).unwrap();
    assert_eq!(filter.range.end.column, ColumnIndex(2));
    assert!(!filter.criteria.contains_key(&ColumnIndex(1)));
    assert_eq!(
        filter.criteria.get(&ColumnIndex(2)),
        Some(&FilterCriterion::GreaterThan(0.0))
    );
    assert_eq!(api.filter_hidden_rows(sheet), [RowIndex(4)]);

    api.delete_columns(sheet, ColumnIndex(0), 3).unwrap();
    assert!(api.auto_filter(sheet).is_none());
    assert!(api.filter_hidden_rows(sheet).is_empty());
}

#[test]
fn deleting_filter_header_promotes_first_data_row_and_rederives_visibility() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "Status");
    put_text(&mut api, sheet, 1, 0, "keep");
    put_text(&mut api, sheet, 2, 0, "hide");
    let mut criteria = BTreeMap::new();
    criteria.insert(ColumnIndex(0), FilterCriterion::Equals("keep".into()));
    api.set_auto_filter(
        sheet,
        AutoFilter {
            range: range(sheet, 0, 0, 2, 0),
            has_header: true,
            criteria,
        },
    )
    .unwrap();
    assert_eq!(api.filter_hidden_rows(sheet), [RowIndex(2)]);
    let before = state(&api, sheet);

    api.delete_rows(sheet, RowIndex(0), 1).unwrap();

    let filter = api.auto_filter(sheet).unwrap();
    assert_eq!(filter.range.start.row, RowIndex(0));
    assert_eq!(filter.range.end.row, RowIndex(1));
    assert!(!filter.has_header);
    assert_eq!(api.filter_hidden_rows(sheet), [RowIndex(1)]);
    let after = state(&api, sheet);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    assert_eq!(api.filter_hidden_rows(sheet), [RowIndex(2)]);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);
    assert_eq!(api.filter_hidden_rows(sheet), [RowIndex(1)]);
}

#[test]
fn merge_moves_when_shifted_as_a_whole_and_intersections_reject_atomically() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    api.merge_cells(sheet, range(sheet, 2, 1, 3, 2)).unwrap();
    let before = state(&api, sheet);

    api.insert_rows(sheet, RowIndex(1), 1).unwrap();

    let after = state(&api, sheet);
    assert_eq!(after.merges, [range(sheet, 3, 1, 4, 2)]);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), before);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), after);

    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    api.merge_cells(sheet, range(sheet, 1, 0, 3, 1)).unwrap();
    let before = api.document().snapshot();
    let history = api.history_stats();
    let error = api.insert_rows(sheet, RowIndex(2), 1).unwrap_err();
    assert_structural_rejection(&error);
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history);

    let error = api.delete_columns(sheet, ColumnIndex(1), 1).unwrap_err();
    assert_structural_rejection(&error);
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history);
}

#[test]
fn empty_sheet_structural_noops_and_delete_all_materialized_rows_are_reversible() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    let before = api.document().snapshot();
    let history = api.history_stats();

    api.insert_rows(sheet, RowIndex(4), 1).unwrap();
    api.delete_rows(sheet, RowIndex(4), 1).unwrap();
    api.insert_columns(sheet, ColumnIndex(4), 1).unwrap();
    api.delete_columns(sheet, ColumnIndex(4), 1).unwrap();
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history);

    put(&mut api, sheet, 0, 0, CellInput::Number(1.0));
    put(&mut api, sheet, 1, 1, CellInput::Text("two".into()));
    put(&mut api, sheet, 2, 2, CellInput::Number(3.0));
    api.set_row_dimension(sheet, RowIndex(2), 39.0).unwrap();
    let populated = state(&api, sheet);
    api.delete_rows(sheet, RowIndex(0), 3).unwrap();
    let empty = state(&api, sheet);
    assert!(empty.cells.is_empty());
    assert!(empty.row_dimensions.is_empty());
    api.undo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), populated);
    api.redo_sheet(sheet).unwrap();
    assert_eq!(state(&api, sheet), empty);
}

#[test]
fn invalid_bounds_and_affected_cell_cap_reject_without_mutation() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(1.0));
    let row_limit = api.config().max_document_rows;
    let column_limit = api.config().max_document_columns;
    for operation in 0..6 {
        let before = api.document().snapshot();
        let history = api.history_stats();
        let result = match operation {
            0 => api.insert_rows(sheet, RowIndex(0), 0),
            1 => api.insert_rows(sheet, RowIndex(u32::MAX), 2),
            2 => api.insert_rows(sheet, RowIndex(row_limit), 1),
            3 => api.insert_columns(sheet, ColumnIndex(0), 0),
            4 => api.insert_columns(sheet, ColumnIndex(u32::MAX), 2),
            _ => api.insert_columns(sheet, ColumnIndex(column_limit), 1),
        };
        assert!(result.is_err());
        assert_eq!(api.document().snapshot(), before);
        assert_eq!(api.history_stats(), history);
    }

    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    let mut snapshot = api.document().snapshot();
    snapshot.sheets[0].cells = (0..10_001)
        .map(|row| CellSnapshot {
            address: address(sheet, row, 0),
            input: CellInput::Number(row as f64),
            format: None,
        })
        .collect();
    api.document.workbook = Workbook::from_snapshot(snapshot).unwrap();
    let before = api.document().snapshot();
    let history = api.history_stats();
    let error = api.insert_rows(sheet, RowIndex(0), 1).unwrap_err();
    assert_structural_rejection(&error);
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history);
}

#[test]
fn structural_edit_rejects_when_its_inverse_exceeds_history_budget() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 2, 0, CellInput::Number(5.0));
    api.set_history_limits(HistoryLimits {
        max_entries: 0,
        max_retained_cells: 0,
        max_retained_bytes: 0,
    });
    let before = api.document().snapshot();
    let history = api.history_stats();

    let error = api.insert_rows(sheet, RowIndex(0), 1).unwrap_err();

    assert!(matches!(
        error.kind(),
        ApiErrorKind::Transaction(ApiTransactionError::HistoryBudgetExceeded { .. })
    ));
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history);
}

#[test]
fn structural_edit_invalidates_redo_and_documents_keep_independent_state() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(1.0));
    put(&mut api, sheet, 0, 1, CellInput::Number(2.0));
    api.undo_sheet(sheet).unwrap();
    assert!(api.can_redo());

    api.insert_rows(sheet, RowIndex(0), 1).unwrap();

    assert!(!api.can_redo());
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 1, 0)),
        Some(&CellInput::Number(1.0))
    );

    let mut other = SpreadsheetApi::production();
    let other_sheet = other.document().active_sheet();
    put(&mut other, other_sheet, 0, 0, CellInput::Number(99.0));
    let other_before = other.document().snapshot();
    api.insert_rows(sheet, RowIndex(1), 1).unwrap();
    assert_eq!(other.document().snapshot(), other_before);
}

#[test]
fn structural_owner_replay_preserves_foreign_formula_and_filter_independently() {
    let mut api = SpreadsheetApi::production();
    let first = api.document().active_sheet();
    api.add_sheet(Some("Second".into())).unwrap();
    let second = api.document().active_sheet();
    put(&mut api, first, 0, 0, CellInput::Number(7.0));
    put_formula(&mut api, second, 0, 0, "=Sheet1!A1*2");
    put_text(&mut api, second, 0, 1, "Status");
    put_text(&mut api, second, 1, 1, "keep");
    put_text(&mut api, second, 2, 1, "hide");
    let first_before = state(&api, first);
    let second_before = state(&api, second);
    assert_number(&api, second, 0, 0, 14.0);

    api.insert_rows(first, RowIndex(0), 1).unwrap();
    let first_after = state(&api, first);
    assert_eq!(
        api.document()
            .cell_input(second, address(second, 0, 0))
            .and_then(CellInput::as_formula),
        Some("Sheet1!A2*2")
    );
    let mut criteria = BTreeMap::new();
    criteria.insert(ColumnIndex(1), FilterCriterion::Equals("keep".into()));
    api.set_auto_filter(
        second,
        AutoFilter {
            range: range(second, 0, 1, 2, 1),
            has_header: true,
            criteria,
        },
    )
    .unwrap();
    let second_filtered = state(&api, second);
    assert_eq!(api.filter_hidden_rows(second), [RowIndex(2)]);

    api.undo_sheet(first).unwrap();
    assert_eq!(state(&api, first), first_before);
    assert_eq!(
        api.document()
            .cell_input(second, address(second, 0, 0))
            .and_then(CellInput::as_formula),
        Some("Sheet1!A1*2")
    );
    let mut second_before_with_filter = second_before.clone();
    second_before_with_filter.auto_filter = second_filtered.auto_filter.clone();
    assert_eq!(state(&api, second), second_before_with_filter);
    assert_eq!(api.filter_hidden_rows(second), [RowIndex(2)]);

    api.undo_sheet(second).unwrap();
    assert!(api.auto_filter(second).is_none());
    assert!(api.filter_hidden_rows(second).is_empty());
    assert_eq!(state(&api, second), second_before);
    assert_number(&api, second, 0, 0, 14.0);

    api.redo_sheet(second).unwrap();
    assert_eq!(api.filter_hidden_rows(second), [RowIndex(2)]);
    assert_eq!(state(&api, first), first_before);
    assert_eq!(state(&api, second), second_before_with_filter);

    api.redo_sheet(first).unwrap();
    assert_eq!(state(&api, first), first_after);
    assert_eq!(state(&api, second), second_filtered);
    assert_eq!(api.filter_hidden_rows(second), [RowIndex(2)]);
}

#[test]
fn later_foreign_formula_edit_blocks_targeted_undo_of_cross_sheet_structural_change() {
    let mut api = SpreadsheetApi::production();
    let source = api.document().active_sheet();
    api.add_sheet(Some("Other".into())).unwrap();
    let dependent = api.document().active_sheet();
    put(&mut api, source, 0, 0, CellInput::Number(4.0));
    put_formula(&mut api, dependent, 0, 0, "=Sheet1!A1");
    api.insert_rows(source, RowIndex(0), 1).unwrap();
    put_formula(&mut api, dependent, 1, 0, "=Sheet1!A2+1");
    let before = api.document().snapshot();
    let history = api.history_stats();

    let error = api.undo_sheet(source).unwrap_err();

    assert!(matches!(
        error.kind(),
        ApiErrorKind::Unsupported(ApiUnsupportedError::HistoryDependencyConflict { sheet })
            if *sheet == dependent
    ));
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history);
}

#[test]
fn older_formula_replacement_undo_conflicts_with_later_source_row_insert() {
    let mut api = SpreadsheetApi::production();
    let source = api.document().active_sheet();
    put(&mut api, source, 0, 0, CellInput::Number(7.0));
    api.add_sheet(Some("Other".into())).unwrap();
    let dependent = api.document().active_sheet();
    put_formula(&mut api, dependent, 0, 0, "=Sheet1!A1");
    assert_number(&api, dependent, 0, 0, 7.0);
    put(&mut api, dependent, 0, 0, CellInput::Number(9.0));

    api.insert_rows(source, RowIndex(0), 1).unwrap();
    assert_eq!(
        api.document().cell_input(source, address(source, 1, 0)),
        Some(&CellInput::Number(7.0))
    );
    assert_eq!(
        api.document()
            .cell_input(dependent, address(dependent, 0, 0)),
        Some(&CellInput::Number(9.0))
    );
    let before_rejected_undo = api.document().snapshot();
    let history_before_rejected_undo = api.history_stats();

    let error = api.undo_sheet(dependent).unwrap_err();

    assert!(matches!(
        error.kind(),
        ApiErrorKind::Unsupported(ApiUnsupportedError::HistoryDependencyConflict { sheet })
            if *sheet == source
    ));
    assert_eq!(api.document().snapshot(), before_rejected_undo);
    assert_eq!(api.history_stats(), history_before_rejected_undo);

    api.undo_sheet(source).unwrap();
    assert_eq!(
        api.document().cell_input(source, address(source, 0, 0)),
        Some(&CellInput::Number(7.0))
    );
    api.undo_sheet(dependent).unwrap();
    assert_eq!(
        api.document()
            .cell_input(dependent, address(dependent, 0, 0))
            .and_then(CellInput::as_formula),
        Some("Sheet1!A1")
    );
    assert_number(&api, dependent, 0, 0, 7.0);

    api.redo_sheet(dependent).unwrap();
    assert_eq!(
        api.document()
            .cell_input(dependent, address(dependent, 0, 0)),
        Some(&CellInput::Number(9.0))
    );
    api.redo_sheet(source).unwrap();
    assert_eq!(
        api.document().cell_input(source, address(source, 1, 0)),
        Some(&CellInput::Number(7.0))
    );
    assert_eq!(
        api.document()
            .cell_input(dependent, address(dependent, 0, 0)),
        Some(&CellInput::Number(9.0))
    );
}

#[test]
fn independent_filter_undo_succeeds_before_older_formula_dependency_conflicts() {
    let mut api = SpreadsheetApi::production();
    let source = api.document().active_sheet();
    put(&mut api, source, 0, 0, CellInput::Number(7.0));
    api.add_sheet(Some("Other".into())).unwrap();
    let dependent = api.document().active_sheet();
    put_text(&mut api, dependent, 0, 1, "Status");
    put_text(&mut api, dependent, 1, 1, "keep");
    put_text(&mut api, dependent, 2, 1, "hide");
    put_formula(&mut api, dependent, 0, 0, "=Sheet1!A1");
    put(&mut api, dependent, 0, 0, CellInput::Number(9.0));
    let mut criteria = BTreeMap::new();
    criteria.insert(ColumnIndex(1), FilterCriterion::Equals("keep".into()));
    api.set_auto_filter(
        dependent,
        AutoFilter {
            range: range(dependent, 0, 1, 2, 1),
            has_header: true,
            criteria,
        },
    )
    .unwrap();
    assert_eq!(api.filter_hidden_rows(dependent), [RowIndex(2)]);

    api.insert_rows(source, RowIndex(0), 1).unwrap();
    assert_eq!(
        api.document().cell_input(source, address(source, 1, 0)),
        Some(&CellInput::Number(7.0))
    );
    let history_before_filter_undo = api.history_stats();

    api.undo_sheet(dependent).unwrap();

    assert!(api.auto_filter(dependent).is_none());
    assert!(api.filter_hidden_rows(dependent).is_empty());
    assert_eq!(
        api.document().cell_input(source, address(source, 1, 0)),
        Some(&CellInput::Number(7.0))
    );
    assert_eq!(
        api.document()
            .cell_input(dependent, address(dependent, 0, 0)),
        Some(&CellInput::Number(9.0))
    );
    assert!(api.can_undo_sheet(source));
    assert!(api.can_undo_sheet(dependent));
    assert_eq!(
        api.history_stats().undo_entries + 1,
        history_before_filter_undo.undo_entries
    );

    let before_rejected_undo = api.document().snapshot();
    let history_before_rejected_undo = api.history_stats();
    let error = api.undo_sheet(dependent).unwrap_err();

    assert!(matches!(
        error.kind(),
        ApiErrorKind::Unsupported(ApiUnsupportedError::HistoryDependencyConflict { sheet })
            if *sheet == source
    ));
    assert_eq!(api.document().snapshot(), before_rejected_undo);
    assert_eq!(api.history_stats(), history_before_rejected_undo);
}
