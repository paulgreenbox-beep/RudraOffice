use crate::{
    ApiError, ApiErrorKind, ApiTransactionError, AutoFilter, FilterCriterion, HistoryLimits,
    SortOptions, SpreadsheetApi, SpreadsheetEngine,
};
use sheets_core::{
    CellAddress, CellInput, CellRange, ColumnIndex, CoreError, EngineRequest, EngineResponse,
    RowIndex, SheetId,
};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Debug)]
struct CountingEngine(Rc<Cell<usize>>);

impl SpreadsheetEngine for CountingEngine {
    type Error = CoreError;

    fn evaluate(&mut self, request: EngineRequest) -> Result<EngineResponse, Self::Error> {
        self.0.set(self.0.get().saturating_add(1));
        Ok(EngineResponse::new(request.base_revision(), Vec::new()))
    }
}

fn new_api() -> (SpreadsheetApi<CountingEngine>, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    (SpreadsheetApi::new(CountingEngine(calls.clone())), calls)
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

fn limits(
    max_entries: usize,
    max_retained_cells: usize,
    max_retained_bytes: usize,
) -> HistoryLimits {
    HistoryLimits {
        max_entries,
        max_retained_cells,
        max_retained_bytes,
    }
}

fn sorted_options(sheet: SheetId) -> SortOptions {
    SortOptions {
        range: range(sheet, 0, 0, 2, 1),
        key_column: ColumnIndex(0),
        ascending: true,
        has_header: true,
    }
}

fn populated_sort_api(
    payload: String,
) -> (SpreadsheetApi<CountingEngine>, Rc<Cell<usize>>, SheetId) {
    let (mut api, calls) = new_api();
    let sheet = api.document().active_sheet();
    for (row, column, input) in [
        (0, 0, CellInput::Text("key".into())),
        (0, 1, CellInput::Text("label".into())),
        (1, 0, CellInput::Number(2.0)),
        (1, 1, CellInput::Text(payload)),
        (2, 0, CellInput::Number(1.0)),
        (2, 1, CellInput::Text("small".into())),
    ] {
        api.set_cell_input(sheet, address(sheet, row, column), input)
            .unwrap();
    }
    (api, calls, sheet)
}

fn add_tiny_redo(api: &mut SpreadsheetApi<CountingEngine>, sheet: SheetId) {
    api.set_cell_input(sheet, address(sheet, 100, 20), CellInput::Text("z".into()))
        .unwrap();
    api.undo().unwrap();
}

fn tiny_redo_bytes() -> usize {
    let (mut api, _) = new_api();
    let sheet = api.document().active_sheet();
    add_tiny_redo(&mut api, sheet);
    let stats = api.history_stats();
    assert_eq!(stats.redo_entries, 1);
    stats.retained_bytes
}

fn assert_budget_error(error: &ApiError, expected: HistoryLimits) -> (usize, usize) {
    match error.kind() {
        ApiErrorKind::Transaction(ApiTransactionError::HistoryBudgetExceeded {
            required_cell_endpoints,
            required_bytes,
            limits: actual,
        }) => {
            assert_eq!(*actual, expected);
            (*required_cell_endpoints, *required_bytes)
        }
        other => panic!("expected HistoryBudgetExceeded, got {other:?}"),
    }
}

#[test]
fn sort_byte_rejection_keeps_existing_redo_and_skips_engine_evaluation() {
    let redo_bytes = tiny_redo_bytes();
    let (mut api, calls, sheet) = populated_sort_api("large payload ".repeat(512));
    add_tiny_redo(&mut api, sheet);

    let configured = limits(100, 10_000, redo_bytes);
    api.set_history_limits(configured);
    let before = api.document().snapshot();
    let history_before = api.history_stats();
    assert_eq!(history_before.undo_entries, 0);
    assert_eq!(history_before.redo_entries, 1);
    assert_eq!(history_before.retained_bytes, redo_bytes);
    let calls_before = calls.get();

    let error = api.sort_range(sheet, sorted_options(sheet)).unwrap_err();
    let (endpoints, bytes) = assert_budget_error(&error, configured);
    assert!(endpoints <= configured.max_retained_cells);
    assert!(bytes > configured.max_retained_bytes);
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history_before);
    assert_eq!(calls.get(), calls_before);
}

#[test]
fn sort_endpoint_rejection_keeps_existing_redo_and_skips_engine_evaluation() {
    let (mut api, calls, sheet) = populated_sort_api("large payload".into());
    add_tiny_redo(&mut api, sheet);

    let configured = limits(100, 2, usize::MAX);
    api.set_history_limits(configured);
    let before = api.document().snapshot();
    let history_before = api.history_stats();
    assert_eq!(history_before.undo_entries, 0);
    assert_eq!(history_before.redo_entries, 1);
    assert_eq!(history_before.retained_cells, 2);
    let calls_before = calls.get();

    let error = api.sort_range(sheet, sorted_options(sheet)).unwrap_err();
    let (endpoints, bytes) = assert_budget_error(&error, configured);
    assert!(endpoints > configured.max_retained_cells);
    assert!(bytes <= configured.max_retained_bytes);
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history_before);
    assert_eq!(calls.get(), calls_before);
}

#[test]
fn effective_sort_is_rejected_when_history_entries_are_disabled() {
    let (mut api, calls, sheet) = populated_sort_api("large payload".into());
    let configured = limits(0, usize::MAX, usize::MAX);
    api.set_history_limits(configured);
    let before = api.document().snapshot();
    let history_before = api.history_stats();
    let calls_before = calls.get();

    let error = api.sort_range(sheet, sorted_options(sheet)).unwrap_err();
    let (endpoints, bytes) = assert_budget_error(&error, configured);
    assert!(endpoints <= configured.max_retained_cells);
    assert!(bytes <= configured.max_retained_bytes);
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history_before);
    assert_eq!(calls.get(), calls_before);
}

#[test]
fn effective_sort_fits_single_entry_limit_by_evicting_older_history() {
    let (mut api, calls, sheet) = populated_sort_api("large payload".into());
    let configured = limits(1, 10_000, usize::MAX);
    api.set_history_limits(configured);
    let history_before = api.history_stats();
    assert_eq!(history_before.undo_entries, 1);
    assert_eq!(history_before.redo_entries, 0);
    let calls_before = calls.get();

    api.sort_range(sheet, sorted_options(sheet)).unwrap();

    let history_after = api.history_stats();
    assert_eq!(history_after.undo_entries, 1);
    assert_eq!(history_after.redo_entries, 0);
    assert_eq!(
        history_after.evicted_entries,
        history_before.evicted_entries + 1
    );
    assert_eq!(calls.get(), calls_before + 1);
    assert!(api.can_undo());
}

fn filter_definition(sheet: SheetId, text: String) -> AutoFilter {
    AutoFilter {
        range: range(sheet, 0, 0, 1, 0),
        has_header: false,
        criteria: BTreeMap::from([(ColumnIndex(0), FilterCriterion::Equals(text))]),
    }
}

#[test]
fn filter_definition_rejects_insufficient_bytes_before_engine_evaluation() {
    let (mut api, calls) = new_api();
    let sheet = api.document().active_sheet();
    let configured = limits(10, 0, 0);
    api.set_history_limits(configured);
    let before = api.document().snapshot();
    let history_before = api.history_stats();
    let calls_before = calls.get();

    let error = api
        .set_auto_filter(sheet, filter_definition(sheet, "criterion".repeat(16)))
        .unwrap_err();
    let (endpoints, bytes) = assert_budget_error(&error, configured);
    assert_eq!(endpoints, 0);
    assert!(bytes > configured.max_retained_bytes);
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history_before);
    assert_eq!(calls.get(), calls_before);
}

#[test]
fn filter_definition_rejects_when_history_entries_are_disabled() {
    let (mut api, calls) = new_api();
    let sheet = api.document().active_sheet();
    let configured = limits(0, usize::MAX, usize::MAX);
    api.set_history_limits(configured);
    let before = api.document().snapshot();
    let history_before = api.history_stats();
    let calls_before = calls.get();

    let error = api
        .set_auto_filter(sheet, filter_definition(sheet, "criterion".into()))
        .unwrap_err();
    let (endpoints, bytes) = assert_budget_error(&error, configured);
    assert!(endpoints <= configured.max_retained_cells);
    assert!(bytes <= configured.max_retained_bytes);
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history_before);
    assert_eq!(calls.get(), calls_before);
}

#[test]
fn clear_filter_rejects_when_its_undo_payload_exceeds_the_byte_budget() {
    let (mut api, calls) = new_api();
    let sheet = api.document().active_sheet();
    api.set_auto_filter(sheet, filter_definition(sheet, "criterion".repeat(16)))
        .unwrap();

    let configured = limits(10, usize::MAX, 0);
    api.set_history_limits(configured);
    let before = api.document().snapshot();
    let history_before = api.history_stats();
    let calls_before = calls.get();

    let error = api.clear_auto_filter(sheet).unwrap_err();
    let (endpoints, bytes) = assert_budget_error(&error, configured);
    assert_eq!(endpoints, 0);
    assert!(bytes > configured.max_retained_bytes);
    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history_before);
    assert_eq!(calls.get(), calls_before);
}

#[test]
fn no_op_sort_and_filter_definition_succeed_with_zero_entry_capacity() {
    let (mut api, calls, sheet) = populated_sort_api("small".into());
    let options = sorted_options(sheet);
    api.sort_range(sheet, options.clone()).unwrap();
    let filter = filter_definition(sheet, "criterion".into());
    api.set_auto_filter(sheet, filter.clone()).unwrap();

    let configured = limits(0, 0, 0);
    api.set_history_limits(configured);
    let before = api.document().snapshot();
    let history_before = api.history_stats();
    let calls_before = calls.get();

    api.sort_range(sheet, options).unwrap();
    api.set_auto_filter(sheet, filter).unwrap();

    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history_before);
    assert_eq!(calls.get(), calls_before);

    let (mut blank, blank_calls) = new_api();
    let blank_sheet = blank.document().active_sheet();
    blank.set_history_limits(configured);
    let blank_before = blank.document().snapshot();
    let blank_history_before = blank.history_stats();
    let blank_calls_before = blank_calls.get();
    blank.clear_auto_filter(blank_sheet).unwrap();
    assert_eq!(blank.document().snapshot(), blank_before);
    assert_eq!(blank.history_stats(), blank_history_before);
    assert_eq!(blank_calls.get(), blank_calls_before);
}
