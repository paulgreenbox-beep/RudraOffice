//! RC27 command-boundary regressions. No parallel workbook or GUI-only mutations.
use super::*;

fn book(state: &mut AppState, name: &str) -> RuntimeDocumentId {
    let api = SpreadsheetApi::production();
    let id = state
        .register_document(
            PathBuf::from(name),
            OpenDocument::Sheets(api.document().snapshot()),
        )
        .unwrap();
    state.activate_document(id).unwrap();
    id
}

fn addr(state: &AppState, row: u32, col: u32) -> CellAddress {
    CellAddress::new(
        state.sheets_api.as_ref().unwrap().document().active_sheet(),
        RowIndex(row),
        ColumnIndex(col),
    )
}

fn write(state: &mut AppState, row: u32, col: u32, text: &str) {
    state.sheets_select_cell(addr(state, row, col));
    state.apply_sheets_cell_input(text.into()).unwrap();
}

fn select(state: &mut AppState, a: (u32, u32), b: (u32, u32)) {
    let a = addr(state, a.0, a.1);
    let b = addr(state, b.0, b.1);
    state
        .sheets_view
        .as_mut()
        .unwrap()
        .select_range(a, b)
        .unwrap();
}

fn raw(state: &AppState, row: u32, col: u32) -> String {
    let a = addr(state, row, col);
    state
        .sheets_api
        .as_ref()
        .unwrap()
        .cell_view(a.sheet, a)
        .unwrap()
        .raw_text()
}

fn display(state: &AppState, row: u32, col: u32) -> String {
    let a = addr(state, row, col);
    state
        .sheets_api
        .as_ref()
        .unwrap()
        .cell_view(a.sheet, a)
        .unwrap()
        .display_text()
}

fn seed(state: &mut AppState) {
    for (r, values) in [
        ["Item", "Qty", "Price", "Total"],
        ["A", "1", "10", "=B2*C2"],
        ["B", "2", "20", "=B3*C3"],
        ["C", "3", "30", "=B4*C4"],
    ]
    .into_iter()
    .enumerate()
    {
        for (c, value) in values.into_iter().enumerate() {
            write(state, r as u32, c as u32, value);
        }
    }
}

#[test]
fn structural_delete_range_endpoint_shrinks_and_undo_restores_exact_formula() {
    let mut state = AppState::default();
    let id = book(&mut state, "rc27-native-range.xlsx");
    for (r, value) in ["2", "3", "4"].into_iter().enumerate() {
        write(&mut state, r as u32, 0, value);
    }
    write(&mut state, 7, 5, "=SUM(A$1:$A3)");
    let before = state.sheets_snapshot().unwrap().sheets.clone();
    select(&mut state, (2, 0), (2, 0));
    state
        .apply_ribbon_command(RibbonCommand::DeleteRows)
        .unwrap();
    assert_eq!(raw(&state, 6, 5), "=SUM(A$1:$A2)");
    assert_eq!(display(&state, 6, 5), "5");
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, before);
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Redo)
        .unwrap();
    select(&mut state, (0, 0), (1, 0));
    state
        .apply_ribbon_command(RibbonCommand::DeleteRows)
        .unwrap();
    assert_eq!(raw(&state, 4, 5), "=SUM(#REF!)");
    assert_eq!(display(&state, 4, 5), "#REF!");
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, 6, 5), "=SUM(A$1:$A2)");
}

#[test]
fn structural_rows_use_canonical_formula_engine_and_single_exact_history_entry() {
    let mut state = AppState::default();
    let id = book(&mut state, "rc27-rows.xlsx");
    seed(&mut state);
    select(&mut state, (2, 0), (2, 3));
    let before = state.sheets_snapshot().unwrap().sheets.clone();
    let entries = state
        .sheets_api
        .as_ref()
        .unwrap()
        .history_stats()
        .undo_entries;
    state
        .apply_ribbon_command(RibbonCommand::InsertRows)
        .unwrap();
    assert_eq!(raw(&state, 2, 0), "");
    assert_eq!(raw(&state, 3, 0), "B");
    assert_eq!(raw(&state, 3, 3), "=B4*C4");
    assert_eq!(display(&state, 3, 3), "40");
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .history_stats()
            .undo_entries,
        entries + 1
    );
    let after = state.sheets_snapshot().unwrap().sheets.clone();
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, before);
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Redo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, after);
    select(&mut state, (3, 0), (3, 3));
    state
        .apply_ribbon_command(RibbonCommand::DeleteRows)
        .unwrap();
    assert_eq!(raw(&state, 3, 0), "C");
    assert_eq!(raw(&state, 3, 3), "=B4*C4");
    assert_eq!(display(&state, 3, 3), "90");
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, after);
}

#[test]
fn structural_columns_rewrite_references_and_deleted_reference_is_not_masked() {
    let mut state = AppState::default();
    let id = book(&mut state, "rc27-columns.xlsx");
    seed(&mut state);
    select(&mut state, (1, 2), (3, 2));
    let before = state.sheets_snapshot().unwrap().sheets.clone();
    state
        .apply_ribbon_command(RibbonCommand::InsertColumns)
        .unwrap();
    assert_eq!(raw(&state, 1, 2), "");
    assert_eq!(raw(&state, 1, 3), "10");
    assert_eq!(raw(&state, 1, 4), "=B2*D2");
    assert_eq!(display(&state, 3, 4), "90");
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, before);
    select(&mut state, (1, 1), (3, 1));
    state
        .apply_ribbon_command(RibbonCommand::DeleteColumns)
        .unwrap();
    assert_eq!(display(&state, 1, 2), "#REF!");
    assert!(raw(&state, 1, 2).contains("#REF!"));
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, before);
}

#[test]
fn structural_commands_follow_contiguous_selection_and_clear_search_cursor() {
    let mut state = AppState::default();
    book(&mut state, "rc27-selection.xlsx");
    seed(&mut state);
    let key = state.sheets_tools_key().unwrap();
    state.sheets_tools.entry(key).or_default().last_match = Some(addr(&state, 2, 0));
    select(&mut state, (2, 0), (1, 3));
    state
        .apply_ribbon_command(RibbonCommand::InsertRows)
        .unwrap();
    assert_eq!(raw(&state, 3, 0), "A");
    assert_eq!(raw(&state, 4, 0), "B");
    assert!(state.sheets_tools[&key].last_match.is_none());
    let range = state
        .sheets_view
        .as_ref()
        .unwrap()
        .selection()
        .range()
        .unwrap();
    assert_eq!((range.start.row.0, range.end.row.0), (1, 2));
    assert_eq!((range.start.column.0, range.end.column.0), (0, 3));
    state
        .apply_ribbon_command(RibbonCommand::DeleteRows)
        .unwrap();
    assert_eq!(raw(&state, 1, 0), "A");
    assert_eq!(
        state
            .sheets_view
            .as_ref()
            .unwrap()
            .selection()
            .focus
            .unwrap()
            .row
            .0,
        1
    );
}

#[test]
fn structural_history_and_workbooks_remain_document_isolated() {
    let mut state = AppState::default();
    let a = book(&mut state, "rc27-a.xlsx");
    seed(&mut state);
    select(&mut state, (1, 0), (1, 0));
    let a_before = state.sheets_snapshot().unwrap().sheets.clone();
    state
        .apply_ribbon_command(RibbonCommand::InsertRows)
        .unwrap();
    let a_after = state.sheets_snapshot().unwrap().sheets.clone();
    let b = book(&mut state, "rc27-b.xlsx");
    seed(&mut state);
    let b_before = state.sheets_snapshot().unwrap().sheets.clone();
    select(&mut state, (0, 1), (0, 1));
    state
        .apply_ribbon_command(RibbonCommand::DeleteColumns)
        .unwrap();
    let b_after = state.sheets_snapshot().unwrap().sheets.clone();
    state
        .dispatch_history_command(b, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, b_before);
    state.activate_document(a).unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, a_after);
    state
        .dispatch_history_command(a, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, a_before);
    state
        .dispatch_history_command(a, OfficeHistoryCommand::Redo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, a_after);
    state.activate_document(b).unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, b_before);
    state
        .dispatch_history_command(b, OfficeHistoryCommand::Redo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, b_after);
}

#[test]
fn structural_budget_rejection_preserves_model_history_selection_and_scroll() {
    let mut state = AppState::default();
    book(&mut state, "rc27-budget.xlsx");
    seed(&mut state);
    select(&mut state, (1, 0), (1, 3));
    state
        .sheets_api
        .as_mut()
        .unwrap()
        .set_history_limits(sheets_api::HistoryLimits {
            max_retained_cells: 1,
            ..Default::default()
        });
    let before = state.sheets_api.as_ref().unwrap().document().snapshot();
    let history = state.sheets_api.as_ref().unwrap().history_stats();
    let selection = state.sheets_view.as_ref().unwrap().selection().clone();
    let scroll = state.sheets_view.as_ref().unwrap().scroll_offset();
    let error = state
        .apply_ribbon_command(RibbonCommand::InsertRows)
        .unwrap_err();
    assert!(error.contains("бюджет отмены"), "{error}");
    assert_eq!(
        state.sheets_api.as_ref().unwrap().document().snapshot(),
        before
    );
    assert_eq!(state.sheets_api.as_ref().unwrap().history_stats(), history);
    assert_eq!(state.sheets_view.as_ref().unwrap().selection(), &selection);
    assert_eq!(state.sheets_view.as_ref().unwrap().scroll_offset(), scroll);
}

#[test]
fn structural_commands_are_disabled_for_source_backed_workbooks() {
    let mut state = AppState::default();
    let api = SpreadsheetApi::production();
    let bytes: Arc<[u8]> = Arc::from(api.export_xlsx().unwrap());
    let initial = import_bounded_sheet_window(&bytes, BoundedSheetWindow::initial()).unwrap();
    let id = state
        .register_document_with_source(
            DocumentSource::from_path_with_retained_bytes(
                PathBuf::from("rc27-bounded.xlsx"),
                bytes,
            ),
            OpenDocument::Sheets(initial),
        )
        .unwrap();
    state.activate_document(id).unwrap();
    let before = state.sheets_snapshot().unwrap().clone();
    for command in [
        RibbonCommand::InsertRows,
        RibbonCommand::DeleteRows,
        RibbonCommand::InsertColumns,
        RibbonCommand::DeleteColumns,
    ] {
        assert!(!state.ribbon_command_available(command));
        let error = state.apply_ribbon_command(command).unwrap_err();
        assert!(error.contains("полностью загруженной"), "{error}");
    }
    assert_eq!(state.sheets_snapshot().unwrap(), &before);
}

#[test]
fn structural_frozen_boundary_rejection_is_controlled_and_atomic() {
    let mut state = AppState::default();
    book(&mut state, "rc27-frozen.xlsx");
    seed(&mut state);
    let before = state.sheets_snapshot().unwrap().clone();
    for frozen in [
        sheets_grid::FrozenPanes::with(1, 0),
        sheets_grid::FrozenPanes::with(0, 1),
    ] {
        state
            .sheets_view
            .as_mut()
            .unwrap()
            .handle_event(UiEvent::SetFrozenPanes(frozen))
            .unwrap();
        for target in [(0, 0), (1, 1), (2, 2)] {
            select(&mut state, target, target);
            for command in [
                RibbonCommand::InsertRows,
                RibbonCommand::DeleteRows,
                RibbonCommand::InsertColumns,
                RibbonCommand::DeleteColumns,
            ] {
                assert!(!state.ribbon_command_available(command));
                let error = state.apply_ribbon_command(command).unwrap_err();
                assert!(error.contains("закреплённых"), "{error}");
                assert_eq!(state.sheets_snapshot().unwrap(), &before);
            }
        }
    }
}

#[test]
fn structural_geometry_replaces_shifted_dimensions_and_hides_columns() {
    let mut state = AppState::default();
    let id = book(&mut state, "rc27-geometry.xlsx");
    seed(&mut state);
    let sheet = addr(&state, 0, 0).sheet;
    let api = state.sheets_api.as_mut().unwrap();
    api.set_row_dimension(sheet, RowIndex(2), 48.0).unwrap();
    api.set_column_dimension(sheet, ColumnIndex(2), 180.0)
        .unwrap();
    api.execute(
        sheets_core::ChangeSet::new().with(sheets_core::Change::SetManualHiddenColumns {
            sheet,
            columns: vec![ColumnIndex(4)],
        }),
    )
    .unwrap();
    state.commit_sheets_api_snapshot().unwrap();
    select(&mut state, (1, 0), (1, 0));
    state
        .apply_ribbon_command(RibbonCommand::InsertRows)
        .unwrap();
    let frame = state.sheets_frame().unwrap();
    let moved = frame
        .cells
        .iter()
        .find(|cell| cell.address == addr(&state, 3, 0))
        .unwrap();
    let old = frame
        .cells
        .iter()
        .find(|cell| cell.address == addr(&state, 2, 0))
        .unwrap();
    assert_eq!(moved.rect.h, 48.0);
    assert_ne!(old.rect.h, 48.0);
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    select(&mut state, (0, 1), (0, 1));
    state
        .apply_ribbon_command(RibbonCommand::InsertColumns)
        .unwrap();
    let frame = state.sheets_frame().unwrap();
    assert!(!frame.column_headers.iter().any(|header| header.index == 5));
    let moved = frame
        .cells
        .iter()
        .find(|cell| cell.address == addr(&state, 1, 3))
        .unwrap();
    let old = frame
        .cells
        .iter()
        .find(|cell| cell.address == addr(&state, 1, 2))
        .unwrap();
    assert_eq!(moved.rect.w, 180.0);
    assert_ne!(old.rect.w, 180.0);
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert!(
        state
            .sheets_view
            .as_ref()
            .unwrap()
            .is_column_hidden(ColumnIndex(4))
    );
    assert!(
        !state
            .sheets_view
            .as_ref()
            .unwrap()
            .is_column_hidden(ColumnIndex(5))
    );
}

#[test]
fn structural_merges_shift_safely_or_reject_before_change() {
    let mut state = AppState::default();
    let id = book(&mut state, "rc27-merges.xlsx");
    write(&mut state, 2, 0, "kept");
    select(&mut state, (2, 0), (3, 1));
    state
        .apply_ribbon_command(RibbonCommand::MergeSelection)
        .unwrap();
    select(&mut state, (0, 2), (0, 2));
    let before = state.sheets_snapshot().unwrap().sheets.clone();
    state
        .apply_ribbon_command(RibbonCommand::InsertRows)
        .unwrap();
    let after = state.sheets_snapshot().unwrap().sheets.clone();
    assert_eq!(after[0].merges[0].start.row.0, 3);
    assert_eq!(after[0].merges[0].end.row.0, 4);
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, before);
    // Select an unmerged cell in an intersecting row, not the expanded merge.
    select(&mut state, (3, 2), (3, 2));
    let history = state.sheets_api.as_ref().unwrap().history_stats();
    let error = state
        .apply_ribbon_command(RibbonCommand::DeleteRows)
        .unwrap_err();
    assert!(error.contains("безопасно"), "{error}");
    assert_eq!(state.sheets_snapshot().unwrap().sheets, before);
    assert_eq!(state.sheets_api.as_ref().unwrap().history_stats(), history);
}

#[test]
fn structural_filtered_insert_chooses_visible_survivor_without_doubling_next_count() {
    let mut state = AppState::default();
    book(&mut state, "rc27-filtered-selection.xlsx");
    seed(&mut state);
    select(&mut state, (0, 0), (3, 3));
    state.update_sheets_data_draft(Some("A".into()), Some(true), Some(0), Some("A".into()));
    state
        .apply_ribbon_command(RibbonCommand::ApplyAutoFilter)
        .unwrap();
    select(&mut state, (1, 0), (1, 3));
    state
        .apply_ribbon_command(RibbonCommand::InsertRows)
        .unwrap();
    let view = state.sheets_view.as_ref().unwrap();
    assert!(view.is_row_hidden(RowIndex(1)));
    assert_eq!(view.selection().focus, Some(addr(&state, 2, 0)));
    let range = view.selection().range().unwrap();
    assert_eq!(range.start.row, RowIndex(2));
    assert_eq!(range.end.row, RowIndex(2));
    state
        .apply_ribbon_command(RibbonCommand::InsertRows)
        .unwrap();
    assert_eq!(raw(&state, 3, 0), "A");
    assert_eq!(raw(&state, 4, 0), "B");
}

#[test]
fn structural_filter_columns_and_hidden_rows_are_synchronized_immediately() {
    let mut state = AppState::default();
    let id = book(&mut state, "rc27-filter.xlsx");
    seed(&mut state);
    select(&mut state, (0, 0), (3, 3));
    state.update_sheets_data_draft(Some("A".into()), Some(true), Some(0), Some("A".into()));
    state
        .apply_ribbon_command(RibbonCommand::ApplyAutoFilter)
        .unwrap();
    assert!(
        state
            .sheets_view
            .as_ref()
            .unwrap()
            .is_row_hidden(RowIndex(2))
    );
    let before = state.sheets_snapshot().unwrap().sheets.clone();
    select(&mut state, (1, 0), (1, 0));
    state
        .apply_ribbon_command(RibbonCommand::InsertColumns)
        .unwrap();
    let filter = state.sheets_snapshot().unwrap().sheets[0]
        .auto_filter
        .as_ref()
        .unwrap();
    assert_eq!(filter.range.start.column, ColumnIndex(1));
    assert_eq!(filter.range.end.column, ColumnIndex(4));
    assert!(filter.criteria.contains_key(&ColumnIndex(1)));
    assert!(!filter.criteria.contains_key(&ColumnIndex(0)));
    assert_eq!(raw(&state, 1, 1), "A");
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, before);
    select(&mut state, (1, 0), (1, 0));
    state
        .apply_ribbon_command(RibbonCommand::DeleteColumns)
        .unwrap();
    let filter = state.sheets_snapshot().unwrap().sheets[0]
        .auto_filter
        .as_ref()
        .unwrap();
    assert!(filter.criteria.is_empty());
    assert!(
        !state
            .sheets_view
            .as_ref()
            .unwrap()
            .is_row_hidden(RowIndex(2))
    );
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets, before);
}

#[test]
fn structural_cross_sheet_history_preserves_later_independent_filter() {
    let mut state = AppState::default();
    let id = book(&mut state, "rc27-cross-sheet.xlsx");
    write(&mut state, 0, 0, "10");
    state.apply_ribbon_command(RibbonCommand::AddSheet).unwrap();
    write(&mut state, 0, 0, "=Sheet1!A1");
    write(&mut state, 1, 0, "visible");
    state.select_sheet_index(0);
    select(&mut state, (0, 0), (0, 0));
    state
        .apply_ribbon_command(RibbonCommand::InsertRows)
        .unwrap();
    state.select_sheet_index(1);
    assert_eq!(raw(&state, 0, 0), "=Sheet1!A2");
    select(&mut state, (0, 0), (1, 0));
    state.update_sheets_data_draft(
        Some("A".into()),
        Some(false),
        Some(0),
        Some("visible".into()),
    );
    state
        .apply_ribbon_command(RibbonCommand::ApplyAutoFilter)
        .unwrap();
    let other_filter = state.sheets_snapshot().unwrap().sheets[1]
        .auto_filter
        .clone();
    state.select_sheet_index(0);
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, 0, 0), "10");
    assert_eq!(
        state.sheets_snapshot().unwrap().sheets[1].auto_filter,
        other_filter
    );
    state.select_sheet_index(1);
    assert_eq!(raw(&state, 0, 0), "=Sheet1!A1");
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert!(
        state.sheets_snapshot().unwrap().sheets[1]
            .auto_filter
            .is_none()
    );
}
