use crate::*;
use sheets_core::{
    AutoFilter, CellAddress, CellError, CellFormat, CellInput, CellRange, CellValue, Change,
    ChangeSet, ColumnIndex, CoreError, EngineRequest, EngineResponse, FilterCriterion, RowIndex,
    SheetId, SpreadsheetEngine,
};
use std::collections::BTreeMap;

#[test]
fn filter_sort_clear_and_history_rederive_formula_visibility_and_keep_manual_rows() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "Qty");
    for (row, qty) in [(1, 3.0), (2, 1.0), (3, 2.0)] {
        put(&mut api, sheet, row, 0, CellInput::Number(qty));
        put(
            &mut api,
            sheet,
            row,
            1,
            CellInput::formula(format!("=A{}*10", row + 1)).unwrap(),
        );
    }
    api.set_manual_hidden_rows(sheet, vec![RowIndex(0)])
        .unwrap();
    let rect = range(sheet, 0, 0, 3, 1);
    api.set_auto_filter(
        sheet,
        filter(
            rect.clone(),
            true,
            [(1, FilterCriterion::GreaterThan(15.0))],
        ),
    )
    .unwrap();
    assert_eq!(api.filter_hidden_rows(sheet), vec![RowIndex(2)]);
    api.sort_range(sheet, sort_options(rect, 0, true, true))
        .unwrap();
    assert_eq!(api.filter_hidden_rows(sheet), vec![RowIndex(1)]);
    assert_eq!(api.hidden_rows(sheet), vec![RowIndex(0), RowIndex(1)]);
    api.clear_range(
        sheet,
        range(sheet, 2, 0, 2, 0),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    assert_eq!(
        api.filter_hidden_rows(sheet),
        vec![RowIndex(1), RowIndex(2)]
    );
    api.undo_sheet(sheet).unwrap();
    assert_eq!(api.filter_hidden_rows(sheet), vec![RowIndex(1)]);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(api.filter_hidden_rows(sheet), vec![RowIndex(2)]);
    api.redo_sheet(sheet).unwrap();
    api.redo_sheet(sheet).unwrap();
    assert_eq!(
        api.filter_hidden_rows(sheet),
        vec![RowIndex(1), RowIndex(2)]
    );
    api.clear_auto_filter(sheet).unwrap();
    assert_eq!(api.hidden_rows(sheet), vec![RowIndex(0)]);
    api.undo_sheet(sheet).unwrap();
    assert_eq!(
        api.hidden_rows(sheet),
        vec![RowIndex(0), RowIndex(1), RowIndex(2)]
    );
    api.redo_sheet(sheet).unwrap();
    assert_eq!(api.hidden_rows(sheet), vec![RowIndex(0)]);
}

#[test]
fn unique_filter_values_are_canonical_and_bounded_by_text_bytes() {
    let mut api = api();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "Ж");
    put_text(&mut api, sheet, 1, 0, "Ж");
    api.set_auto_filter(sheet, filter(range(sheet, 0, 0, 2, 0), false, []))
        .unwrap();
    assert_eq!(
        api.filter_unique_values(sheet, ColumnIndex(0)).unwrap(),
        vec![String::new(), "Ж".into()]
    );
    put_text(&mut api, sheet, 0, 0, &"x".repeat(MAX_FILTER_TEXT_BYTES));
    let before = api.document().snapshot();
    let history = api.history_stats();
    assert!(api.filter_unique_values(sheet, ColumnIndex(0)).is_err());
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history);
}

#[derive(Debug, Default)]
struct TestEngine;

impl SpreadsheetEngine for TestEngine {
    type Error = CoreError;

    fn evaluate(&mut self, request: EngineRequest) -> Result<EngineResponse, Self::Error> {
        Ok(EngineResponse::new(request.base_revision(), Vec::new()))
    }
}

fn api() -> SpreadsheetApi<TestEngine> {
    SpreadsheetApi::new(TestEngine)
}

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

fn sort_options(
    range: CellRange,
    key_column: u32,
    ascending: bool,
    has_header: bool,
) -> SortOptions {
    SortOptions {
        range,
        key_column: ColumnIndex(key_column),
        ascending,
        has_header,
    }
}

fn filter(
    range: CellRange,
    has_header: bool,
    criteria: impl IntoIterator<Item = (u32, FilterCriterion)>,
) -> AutoFilter {
    AutoFilter {
        range,
        has_header,
        criteria: criteria
            .into_iter()
            .map(|(column, criterion)| (ColumnIndex(column), criterion))
            .collect(),
    }
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
    put(api, sheet, row, column, CellInput::Text(text.into()));
}

fn text_at<E: SpreadsheetEngine>(
    api: &SpreadsheetApi<E>,
    sheet: SheetId,
    row: u32,
    column: u32,
) -> String {
    match api.document.cell_input(sheet, address(sheet, row, column)) {
        Some(CellInput::Text(value)) => value.clone(),
        other => panic!("expected text at ({row}, {column}), got {other:?}"),
    }
}

fn labels<E: SpreadsheetEngine>(
    api: &SpreadsheetApi<E>,
    sheet: SheetId,
    rows: &[u32],
) -> Vec<String> {
    rows.iter()
        .map(|row| text_at(api, sheet, *row, 0))
        .collect()
}

fn hidden_rows<E: SpreadsheetEngine>(api: &SpreadsheetApi<E>, sheet: SheetId) -> Vec<u32> {
    api.filter_hidden_rows(sheet)
        .into_iter()
        .map(|row| row.0)
        .collect()
}

fn sheet_snapshot<E: SpreadsheetEngine>(
    api: &SpreadsheetApi<E>,
    sheet: SheetId,
) -> sheets_core::SheetSnapshot {
    api.document
        .snapshot()
        .sheets
        .into_iter()
        .find(|snapshot| snapshot.id == sheet)
        .unwrap()
}

fn numeric_table() -> (SpreadsheetApi<TestEngine>, SheetId) {
    let mut api = api();
    let sheet = api.document.active_sheet();
    put_text(&mut api, sheet, 0, 0, "Item");
    put_text(&mut api, sheet, 0, 1, "Qty");
    put_text(&mut api, sheet, 1, 0, "zeta");
    put(&mut api, sheet, 1, 1, CellInput::Number(2.0));
    put_text(&mut api, sheet, 2, 0, "alpha");
    put(&mut api, sheet, 2, 1, CellInput::Number(2.0));
    put_text(&mut api, sheet, 3, 0, "mu");
    put(&mut api, sheet, 3, 1, CellInput::Number(1.0));
    (api, sheet)
}

fn text_filter_table() -> (SpreadsheetApi<TestEngine>, SheetId) {
    let mut api = api();
    let sheet = api.document.active_sheet();
    put_text(&mut api, sheet, 0, 0, "Имя");
    put_text(&mut api, sheet, 0, 1, "Количество");
    for (row, name, quantity) in [
        (1, "Альфа", 10.0),
        (2, "Бета", 20.0),
        (3, "Альфа", 30.0),
        (4, "альфа", 40.0),
    ] {
        put_text(&mut api, sheet, row, 0, name);
        put(&mut api, sheet, row, 1, CellInput::Number(quantity));
    }
    (api, sheet)
}

#[test]
fn sort_keeps_header_and_equal_keys_stable_in_ascending_order() {
    let (mut api, sheet) = numeric_table();
    api.sort_range(sheet, sort_options(range(sheet, 0, 0, 3, 1), 1, true, true))
        .unwrap();

    assert_eq!(text_at(&api, sheet, 0, 0), "Item");
    assert_eq!(labels(&api, sheet, &[1, 2, 3]), ["mu", "zeta", "alpha"]);
}

#[test]
fn sort_keeps_equal_keys_stable_in_descending_order() {
    let (mut api, sheet) = numeric_table();
    api.sort_range(
        sheet,
        sort_options(range(sheet, 0, 0, 3, 1), 1, false, true),
    )
    .unwrap();

    assert_eq!(text_at(&api, sheet, 0, 0), "Item");
    assert_eq!(labels(&api, sheet, &[1, 2, 3]), ["zeta", "alpha", "mu"]);
}

#[test]
fn sort_keeps_empty_and_formula_empty_keys_last_in_both_directions() {
    for (ascending, expected) in [
        (true, ["one", "two", "empty", "formula-empty"]),
        (false, ["two", "one", "empty", "formula-empty"]),
    ] {
        let mut api = SpreadsheetApi::production();
        let sheet = api.document.active_sheet();
        put_text(&mut api, sheet, 0, 0, "empty");
        put_text(&mut api, sheet, 1, 0, "formula-empty");
        put(&mut api, sheet, 1, 1, CellInput::formula("=\"\"").unwrap());
        put_text(&mut api, sheet, 2, 0, "two");
        put(&mut api, sheet, 2, 1, CellInput::Number(2.0));
        put_text(&mut api, sheet, 3, 0, "one");
        put(&mut api, sheet, 3, 1, CellInput::Number(1.0));

        api.sort_range(
            sheet,
            sort_options(range(sheet, 0, 0, 3, 1), 1, ascending, false),
        )
        .unwrap();

        assert_eq!(labels(&api, sheet, &[0, 1, 2, 3]), expected);
    }
}

#[test]
fn sort_uses_fixed_type_bands_for_both_directions() {
    for ascending in [true, false] {
        let mut api = SpreadsheetApi::production();
        let sheet = api.document.active_sheet();
        for (row, label) in ["number", "text", "boolean", "error", "blank"]
            .into_iter()
            .enumerate()
        {
            put_text(&mut api, sheet, row as u32, 0, label);
        }
        put(&mut api, sheet, 0, 1, CellInput::Number(5.0));
        put_text(&mut api, sheet, 1, 1, "текст");
        put(&mut api, sheet, 2, 1, CellInput::Boolean(true));
        put(&mut api, sheet, 3, 1, CellInput::formula("=1/0").unwrap());

        api.sort_range(
            sheet,
            sort_options(range(sheet, 0, 0, 4, 1), 1, ascending, false),
        )
        .unwrap();

        assert_eq!(
            labels(&api, sheet, &[0, 1, 2, 3, 4]),
            ["number", "text", "boolean", "error", "blank"]
        );
        assert_eq!(
            api.document.cell_value(sheet, address(sheet, 3, 1)),
            Some(&CellValue::Error(CellError::Div0))
        );
    }
}

#[test]
fn sort_groups_dates_percentages_and_times_with_numbers() {
    let mut api = api();
    let sheet = api.document.active_sheet();
    for (row, label) in ["date", "percent", "time", "number"]
        .into_iter()
        .enumerate()
    {
        put_text(&mut api, sheet, row as u32, 0, label);
    }
    put(&mut api, sheet, 0, 1, CellInput::Date(2.0));
    put(&mut api, sheet, 1, 1, CellInput::Percent(0.5));
    put(&mut api, sheet, 2, 1, CellInput::Time(0.25));
    put(&mut api, sheet, 3, 1, CellInput::Number(1.0));

    api.sort_range(
        sheet,
        sort_options(range(sheet, 0, 0, 3, 1), 1, true, false),
    )
    .unwrap();

    assert_eq!(
        labels(&api, sheet, &[0, 1, 2, 3]),
        ["time", "percent", "number", "date"]
    );
}

#[test]
fn sort_moves_cell_formats_with_rows_and_leaves_physical_row_state_in_place() {
    let mut api = api();
    let sheet = api.document.active_sheet();
    put_text(&mut api, sheet, 0, 0, "Item");
    put_text(&mut api, sheet, 0, 1, "Qty");
    put_text(&mut api, sheet, 1, 0, "middle");
    put(&mut api, sheet, 1, 1, CellInput::Number(2.0));
    put_text(&mut api, sheet, 2, 0, "first");
    put(&mut api, sheet, 2, 1, CellInput::Number(1.0));
    put_text(&mut api, sheet, 3, 0, "last");
    put(&mut api, sheet, 3, 1, CellInput::Number(3.0));
    let format = CellFormat {
        background: Some(sheets_core::Color::new(255, 255, 0, 255)),
        ..CellFormat::default()
    };
    api.set_cell_format(
        sheet,
        address(sheet, 2, 0),
        format.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    api.set_row_dimension(sheet, RowIndex(2), 44.0).unwrap();
    api.set_manual_hidden_rows(sheet, vec![RowIndex(2)])
        .unwrap();

    api.sort_range(sheet, sort_options(range(sheet, 0, 0, 3, 1), 1, true, true))
        .unwrap();

    assert_eq!(labels(&api, sheet, &[1, 2, 3]), ["first", "middle", "last"]);
    assert_eq!(
        api.document.cell_format(sheet, address(sheet, 1, 0)),
        Some(&format)
    );
    let snapshot = sheet_snapshot(&api, sheet);
    assert_eq!(snapshot.row_dimensions.len(), 1);
    assert_eq!(snapshot.row_dimensions[0].row, RowIndex(2));
    assert_eq!(snapshot.row_dimensions[0].height, 44.0);
    assert_eq!(snapshot.manual_hidden_rows, [RowIndex(2)]);
}

#[test]
fn sort_rebases_formulas_and_recalculates_values_after_source_rows_move() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document.active_sheet();
    for (column, title) in ["Name", "Qty", "Price", "Total"].into_iter().enumerate() {
        put_text(&mut api, sheet, 0, column as u32, title);
    }
    put_text(&mut api, sheet, 1, 0, "Apple");
    put(&mut api, sheet, 1, 1, CellInput::Number(2.0));
    put(&mut api, sheet, 1, 2, CellInput::Number(5.0));
    put(&mut api, sheet, 1, 3, CellInput::formula("=B2*C2").unwrap());
    put_text(&mut api, sheet, 2, 0, "Pear");
    put(&mut api, sheet, 2, 1, CellInput::Number(1.0));
    put(&mut api, sheet, 2, 2, CellInput::Number(7.0));
    put(&mut api, sheet, 2, 3, CellInput::formula("=B3*C3").unwrap());

    api.sort_range(sheet, sort_options(range(sheet, 0, 0, 2, 3), 1, true, true))
        .unwrap();

    assert_eq!(text_at(&api, sheet, 1, 0), "Pear");
    assert_eq!(text_at(&api, sheet, 2, 0), "Apple");
    assert!(matches!(
        api.document.cell_input(sheet, address(sheet, 1, 3)),
        Some(CellInput::Formula(formula)) if formula == "B2*C2"
    ));
    assert!(matches!(
        api.document.cell_input(sheet, address(sheet, 2, 3)),
        Some(CellInput::Formula(formula)) if formula == "B3*C3"
    ));
    assert_eq!(
        api.document.cell_value(sheet, address(sheet, 1, 3)),
        Some(&CellValue::Number(7.0))
    );
    assert_eq!(
        api.document.cell_value(sheet, address(sheet, 2, 3)),
        Some(&CellValue::Number(10.0))
    );
}

#[test]
fn sort_rejects_bad_keys_oversized_ranges_and_merges_without_mutation() {
    let mut api = api();
    let sheet = api.document.active_sheet();
    put_text(&mut api, sheet, 0, 0, "Header");
    put_text(&mut api, sheet, 1, 0, "one");
    put_text(&mut api, sheet, 2, 0, "two");
    api.merge_cells(sheet, range(sheet, 1, 0, 1, 1)).unwrap();
    let before = api.document.snapshot();
    let history = api.history_stats();

    let bad_key = sort_options(range(sheet, 0, 0, 2, 1), 2, true, true);
    let oversized = sort_options(range(sheet, 0, 0, 100, 100), 0, true, false);
    let intersects_merge = sort_options(range(sheet, 0, 0, 2, 1), 0, true, true);
    for options in [bad_key, oversized, intersects_merge] {
        assert!(api.sort_range(sheet, options).is_err());
        assert_eq!(api.document.snapshot(), before);
        assert_eq!(api.history_stats(), history);
    }
}

#[test]
fn text_filter_variants_match_case_sensitive_unicode_values() {
    let cases = [
        (
            FilterCriterion::Values(vec!["Альфа".into(), "Бета".into()]),
            vec![4],
        ),
        (FilterCriterion::Equals("Альфа".into()), vec![2, 4]),
        (FilterCriterion::NotEquals("Альфа".into()), vec![1, 3]),
        (FilterCriterion::Contains("Аль".into()), vec![2, 4]),
    ];

    for (criterion, expected_hidden) in cases {
        let (mut api, sheet) = text_filter_table();
        api.set_auto_filter(
            sheet,
            filter(range(sheet, 0, 0, 4, 1), true, [(0, criterion)]),
        )
        .unwrap();
        assert_eq!(hidden_rows(&api, sheet), expected_hidden);
    }
}

#[test]
fn filter_criteria_are_anded_across_columns() {
    let (mut api, sheet) = text_filter_table();
    api.set_auto_filter(
        sheet,
        filter(
            range(sheet, 0, 0, 4, 1),
            true,
            [
                (0, FilterCriterion::Equals("Альфа".into())),
                (1, FilterCriterion::GreaterThan(15.0)),
            ],
        ),
    )
    .unwrap();

    assert_eq!(hidden_rows(&api, sheet), [1, 2, 4]);
}

#[test]
fn blank_filters_include_formula_empty_text_but_exclude_nonempty_cells() {
    for (criterion, expected_hidden) in [
        (FilterCriterion::Blank, vec![2]),
        (FilterCriterion::NonBlank, vec![1, 3, 4]),
    ] {
        let mut api = SpreadsheetApi::production();
        let sheet = api.document.active_sheet();
        put_text(&mut api, sheet, 0, 0, "Value");
        put(&mut api, sheet, 1, 0, CellInput::formula("=\"\"").unwrap());
        put(&mut api, sheet, 2, 0, CellInput::Number(0.0));
        put_text(&mut api, sheet, 4, 0, "");

        api.set_auto_filter(
            sheet,
            filter(range(sheet, 0, 0, 4, 0), true, [(0, criterion)]),
        )
        .unwrap();

        assert_eq!(hidden_rows(&api, sheet), expected_hidden);
    }
}

#[test]
fn numeric_filter_variants_do_not_coerce_text_numbers() {
    let cases = [
        (FilterCriterion::NumberEquals(10.0), vec![2, 3, 4]),
        (FilterCriterion::GreaterThan(10.0), vec![1, 2, 4]),
        (FilterCriterion::LessThan(10.0), vec![1, 2, 3]),
    ];

    for (criterion, expected_hidden) in cases {
        let (mut api, sheet) = text_filter_table();
        put(&mut api, sheet, 1, 1, CellInput::Number(10.0));
        put_text(&mut api, sheet, 2, 1, "10");
        put(&mut api, sheet, 3, 1, CellInput::Number(20.0));
        put(&mut api, sheet, 4, 1, CellInput::Number(5.0));
        api.set_auto_filter(
            sheet,
            filter(range(sheet, 0, 1, 4, 1), true, [(1, criterion)]),
        )
        .unwrap();

        assert_eq!(hidden_rows(&api, sheet), expected_hidden);
    }
}

#[test]
fn filter_is_one_sheet_history_entry_and_reevaluates_current_values() {
    let mut api = api();
    let sheet = api.document.active_sheet();
    put_text(&mut api, sheet, 0, 0, "Name");
    put_text(&mut api, sheet, 1, 0, "keep");
    put_text(&mut api, sheet, 2, 0, "drop");
    api.set_manual_hidden_rows(sheet, vec![RowIndex(2)])
        .unwrap();

    let entries_before_filter = api.history_stats().undo_entries;
    api.set_auto_filter(
        sheet,
        filter(
            range(sheet, 0, 0, 2, 0),
            true,
            [(0, FilterCriterion::Equals("keep".into()))],
        ),
    )
    .unwrap();
    assert_eq!(api.history_stats().undo_entries, entries_before_filter + 1);
    assert_eq!(hidden_rows(&api, sheet), [2]);

    put_text(&mut api, sheet, 2, 0, "keep");
    assert!(hidden_rows(&api, sheet).is_empty());
    assert!(api.can_undo_sheet(sheet));
    api.undo_sheet(sheet).unwrap();
    assert_eq!(hidden_rows(&api, sheet), [2]);
    api.redo_sheet(sheet).unwrap();
    assert!(hidden_rows(&api, sheet).is_empty());

    api.clear_auto_filter(sheet).unwrap();
    assert!(hidden_rows(&api, sheet).is_empty());
    assert_eq!(
        sheet_snapshot(&api, sheet).manual_hidden_rows,
        [RowIndex(2)]
    );
    assert!(api.can_undo_sheet(sheet));
    api.undo_sheet(sheet).unwrap();
    assert!(sheet_snapshot(&api, sheet).auto_filter.is_some());
    assert!(hidden_rows(&api, sheet).is_empty());
    api.redo_sheet(sheet).unwrap();
    assert!(sheet_snapshot(&api, sheet).auto_filter.is_none());
    assert_eq!(
        sheet_snapshot(&api, sheet).manual_hidden_rows,
        [RowIndex(2)]
    );
}

#[test]
fn targeted_history_skips_other_sheet_entries() {
    let mut api = api();
    let first = api.document.active_sheet();
    api.add_sheet(Some("Second".into())).unwrap();
    let second = api.document.active_sheet();

    put_text(&mut api, first, 0, 0, "first");
    put(&mut api, first, 0, 1, CellInput::Number(2.0));
    put_text(&mut api, first, 1, 0, "second");
    put(&mut api, first, 1, 1, CellInput::Number(1.0));
    put_text(&mut api, second, 0, 0, "Name");
    put_text(&mut api, second, 1, 0, "keep");
    put_text(&mut api, second, 2, 0, "drop");

    api.sort_range(
        first,
        sort_options(range(first, 0, 0, 1, 1), 1, true, false),
    )
    .unwrap();
    api.set_auto_filter(
        second,
        filter(
            range(second, 0, 0, 2, 0),
            true,
            [(0, FilterCriterion::Equals("keep".into()))],
        ),
    )
    .unwrap();
    assert_eq!(labels(&api, first, &[0, 1]), ["second", "first"]);
    assert_eq!(hidden_rows(&api, second), [2]);

    api.undo_sheet(first).unwrap();
    assert_eq!(labels(&api, first, &[0, 1]), ["first", "second"]);
    assert_eq!(hidden_rows(&api, second), [2]);
    assert!(api.can_redo_sheet(first));
    assert!(api.can_undo_sheet(second));

    api.undo_sheet(second).unwrap();
    assert!(sheet_snapshot(&api, second).auto_filter.is_none());
    assert_eq!(labels(&api, first, &[0, 1]), ["first", "second"]);
    api.redo_sheet(first).unwrap();
    assert_eq!(labels(&api, first, &[0, 1]), ["second", "first"]);
}

#[test]
fn filter_history_is_independent_between_documents() {
    let mut first = api();
    let first_sheet = first.document.active_sheet();
    put_text(&mut first, first_sheet, 0, 0, "Name");
    put_text(&mut first, first_sheet, 1, 0, "keep");
    first
        .set_auto_filter(
            first_sheet,
            filter(
                range(first_sheet, 0, 0, 1, 0),
                true,
                [(0, FilterCriterion::Equals("keep".into()))],
            ),
        )
        .unwrap();

    let second = api();
    let second_sheet = second.document.active_sheet();
    assert!(!second.can_undo());
    assert!(!second.can_undo_sheet(second_sheet));
    assert!(sheet_snapshot(&second, second_sheet).auto_filter.is_none());
    assert!(sheet_snapshot(&first, first_sheet).auto_filter.is_some());
}

#[test]
fn targeted_undo_rejects_ambiguous_multisheet_entries_atomically() {
    let mut api = api();
    let first = api.document.active_sheet();
    api.add_sheet(Some("Second".into())).unwrap();
    let second = api.document.active_sheet();
    let mut changes = ChangeSet::new();
    changes.push(Change::SetCellInput {
        sheet: first,
        address: address(first, 0, 0),
        input: CellInput::Text("one".into()),
    });
    changes.push(Change::SetCellInput {
        sheet: second,
        address: address(second, 0, 0),
        input: CellInput::Text("two".into()),
    });
    api.execute(changes).unwrap();
    let before = api.document.snapshot();
    let history = api.history_stats();

    assert!(api.undo_sheet(first).is_err());
    assert_eq!(api.document.snapshot(), before);
    assert_eq!(api.history_stats(), history);
}

#[test]
fn invalid_filters_leave_snapshot_and_history_unchanged() {
    let mut api = api();
    let sheet = api.document.active_sheet();
    let valid = range(sheet, 0, 0, 1, 1);
    let mut reversed = valid.clone();
    reversed.start.row = RowIndex(1);
    reversed.end.row = RowIndex(0);
    let out_of_range_column = filter(
        valid.clone(),
        true,
        [(2, FilterCriterion::Equals("outside".into()))],
    );
    let non_finite = filter(
        valid.clone(),
        true,
        [(0, FilterCriterion::NumberEquals(f64::NAN))],
    );
    let invalid_geometry = AutoFilter {
        range: reversed,
        has_header: true,
        criteria: BTreeMap::new(),
    };
    let oversized = filter(range(sheet, 0, 0, 100, 100), false, []);
    let cases = [out_of_range_column, non_finite, invalid_geometry, oversized];
    let before = api.document.snapshot();
    let history = api.history_stats();

    for candidate in cases {
        assert!(api.set_auto_filter(sheet, candidate).is_err());
        assert_eq!(api.document.snapshot(), before);
        assert_eq!(api.history_stats(), history);
    }
}

#[test]
fn sort_rejects_an_unevaluated_formula_key_atomically() {
    let mut api = api();
    let sheet = api.document.active_sheet();
    put_text(&mut api, sheet, 0, 0, "first");
    put(&mut api, sheet, 0, 1, CellInput::formula("=1+1").unwrap());
    put_text(&mut api, sheet, 1, 0, "second");
    put(&mut api, sheet, 1, 1, CellInput::formula("=2+2").unwrap());
    let before = api.document.snapshot();
    let history = api.history_stats();

    let error = api
        .sort_range(
            sheet,
            sort_options(range(sheet, 0, 0, 1, 1), 1, true, false),
        )
        .unwrap_err();

    assert!(matches!(
        error.kind(),
        ApiErrorKind::Unsupported(ApiUnsupportedError::SortFormula(_))
    ));
    assert_eq!(api.document.snapshot(), before);
    assert_eq!(api.history_stats(), history);
}

#[test]
fn filter_rejects_aggregate_values_count_over_limit_atomically() {
    let mut api = api();
    let sheet = api.document.active_sheet();
    let values = vec![String::new(); MAX_SORT_FILTER_CELLS as usize];
    let candidate = filter(
        range(sheet, 0, 0, 1, 1),
        false,
        [
            (0, FilterCriterion::Values(values.clone())),
            (1, FilterCriterion::Values(values)),
        ],
    );
    let before = api.document.snapshot();
    let history = api.history_stats();

    assert!(api.set_auto_filter(sheet, candidate).is_err());
    assert_eq!(api.document.snapshot(), before);
    assert_eq!(api.history_stats(), history);
}

#[test]
fn sort_preserves_raw_empty_cells_formats_and_absence_through_undo_redo() {
    let mut api = api();
    let sheet = api.document.active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(2.0));
    put(&mut api, sheet, 0, 1, CellInput::Number(9.0));
    let format = CellFormat {
        background: Some(sheets_core::Color::new(12, 34, 56, 255)),
        ..CellFormat::default()
    };
    api.set_cell_format(
        sheet,
        address(sheet, 0, 1),
        format.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    api.clear_cell(sheet, address(sheet, 0, 1)).unwrap();
    put(&mut api, sheet, 1, 0, CellInput::Number(1.0));
    let original_history = api.history_stats().undo_entries;

    api.sort_range(
        sheet,
        sort_options(range(sheet, 0, 0, 1, 1), 0, true, false),
    )
    .unwrap();

    let absent_payload = address(sheet, 0, 1);
    let empty_payload = address(sheet, 1, 1);
    assert_eq!(api.document.cell_input(sheet, absent_payload), None);
    assert_eq!(api.document.cell_format(sheet, absent_payload), None);
    assert_eq!(
        api.document.cell_input(sheet, empty_payload),
        Some(&CellInput::Empty)
    );
    assert_eq!(
        api.document.cell_format(sheet, empty_payload),
        Some(&format)
    );
    assert_eq!(api.history_stats().undo_entries, original_history + 1);

    api.undo_sheet(sheet).unwrap();
    assert_eq!(
        api.document.cell_input(sheet, address(sheet, 0, 1)),
        Some(&CellInput::Empty)
    );
    assert_eq!(
        api.document.cell_format(sheet, address(sheet, 0, 1)),
        Some(&format)
    );
    assert_eq!(api.document.cell_input(sheet, address(sheet, 1, 1)), None);
    assert_eq!(api.document.cell_format(sheet, address(sheet, 1, 1)), None);

    api.redo_sheet(sheet).unwrap();
    assert_eq!(api.document.cell_input(sheet, absent_payload), None);
    assert_eq!(api.document.cell_format(sheet, absent_payload), None);
    assert_eq!(
        api.document.cell_input(sheet, empty_payload),
        Some(&CellInput::Empty)
    );
    assert_eq!(
        api.document.cell_format(sheet, empty_payload),
        Some(&format)
    );
}

#[test]
fn sort_rejects_relocation_that_introduces_a_broken_reference_before_mutation() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document.active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(2.0));
    put(&mut api, sheet, 1, 0, CellInput::Number(1.0));
    put(&mut api, sheet, 1, 1, CellInput::formula("=A1").unwrap());
    assert_eq!(
        api.document.cell_value(sheet, address(sheet, 1, 1)),
        Some(&CellValue::Number(2.0))
    );
    let before = api.document.snapshot();
    let history = api.history_stats();

    let error = api
        .sort_range(
            sheet,
            sort_options(range(sheet, 0, 0, 1, 1), 0, true, false),
        )
        .unwrap_err();

    assert!(matches!(
        error.kind(),
        ApiErrorKind::Unsupported(ApiUnsupportedError::SortFormula(_))
    ));
    assert_eq!(api.document.snapshot(), before);
    assert_eq!(api.history_stats(), history);
}

#[test]
fn sort_rejects_dynamic_array_formula_before_mutation() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document.active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(2.0));
    put(&mut api, sheet, 1, 0, CellInput::Number(1.0));
    put(&mut api, sheet, 2, 1, CellInput::Number(99.0));
    put(
        &mut api,
        sheet,
        0,
        1,
        CellInput::formula("=SEQUENCE(2)").unwrap(),
    );
    let before = api.document.snapshot();
    let history = api.history_stats();

    let error = api
        .sort_range(
            sheet,
            sort_options(range(sheet, 0, 0, 1, 1), 0, true, false),
        )
        .unwrap_err();

    assert!(matches!(
        error.kind(),
        ApiErrorKind::Unsupported(ApiUnsupportedError::SortFormula(_))
    ));
    assert_eq!(api.document.snapshot(), before);
    assert_eq!(api.history_stats(), history);
}

#[test]
fn sort_orders_distinct_russian_text_keys_with_row_payload_and_history() {
    for ascending in [true, false] {
        let mut api = SpreadsheetApi::production();
        let sheet = api.document().active_sheet();
        for (row, key) in ["Монтаж", "Кабель", "Автоматика", "Лоток"]
            .into_iter()
            .enumerate()
        {
            put_text(&mut api, sheet, row as u32, 0, key);
            put_text(&mut api, sheet, row as u32, 1, &format!("payload-{key}"));
        }
        let original = sheet_snapshot(&api, sheet);
        let history_before = api.history_stats();

        api.sort_range(
            sheet,
            sort_options(range(sheet, 0, 0, 3, 1), 0, ascending, false),
        )
        .unwrap();

        let sorted = sheet_snapshot(&api, sheet);
        let expected = if ascending {
            ["Автоматика", "Кабель", "Лоток", "Монтаж"]
        } else {
            ["Монтаж", "Лоток", "Кабель", "Автоматика"]
        };
        for (row, key) in expected.into_iter().enumerate() {
            assert_eq!(text_at(&api, sheet, row as u32, 0), key);
            assert_eq!(
                text_at(&api, sheet, row as u32, 1),
                format!("payload-{key}")
            );
        }
        assert_eq!(
            api.history_stats().undo_entries,
            history_before.undo_entries + 1
        );

        api.undo_sheet(sheet).unwrap();
        assert_eq!(sheet_snapshot(&api, sheet), original);
        api.redo_sheet(sheet).unwrap();
        assert_eq!(sheet_snapshot(&api, sheet), sorted);
    }
}

#[test]
fn sort_keeps_numeric_band_before_text_and_blank_in_both_directions() {
    for (ascending, expected) in [
        (true, ["two", "ten", "thirty", "text-five", "blank"]),
        (false, ["thirty", "ten", "two", "text-five", "blank"]),
    ] {
        let mut api = SpreadsheetApi::production();
        let sheet = api.document().active_sheet();
        put(&mut api, sheet, 0, 0, CellInput::Number(10.0));
        put_text(&mut api, sheet, 0, 1, "ten");
        put_text(&mut api, sheet, 1, 0, "5");
        put_text(&mut api, sheet, 1, 1, "text-five");
        put_text(&mut api, sheet, 2, 1, "blank");
        put(&mut api, sheet, 3, 0, CellInput::Number(30.0));
        put_text(&mut api, sheet, 3, 1, "thirty");
        put(&mut api, sheet, 4, 0, CellInput::Number(2.0));
        put_text(&mut api, sheet, 4, 1, "two");

        api.sort_range(
            sheet,
            sort_options(range(sheet, 0, 0, 4, 1), 0, ascending, false),
        )
        .unwrap();

        let sorted_payload = (0..5)
            .map(|row| text_at(&api, sheet, row, 1))
            .collect::<Vec<_>>();
        assert_eq!(sorted_payload, expected);
    }
}
