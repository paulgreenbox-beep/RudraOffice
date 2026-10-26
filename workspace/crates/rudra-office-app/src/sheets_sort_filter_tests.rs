//! RC26 application-boundary regressions for canonical Sheets sorting/filtering.
use super::*;

fn add_book(state: &mut AppState, name: &str) -> RuntimeDocumentId {
    let api = SpreadsheetApi::new(IroncalcAdapter::new());
    state
        .register_document(
            PathBuf::from(name),
            OpenDocument::Sheets(api.document().snapshot()),
        )
        .unwrap()
}

fn active_sheet(state: &AppState) -> SheetId {
    state.sheets_api.as_ref().unwrap().document().active_sheet()
}

fn address(state: &AppState, row: u32, column: u32) -> CellAddress {
    CellAddress::new(active_sheet(state), RowIndex(row), ColumnIndex(column))
}

fn raw(state: &AppState, address: CellAddress) -> String {
    state
        .sheets_api
        .as_ref()
        .unwrap()
        .cell_view(address.sheet, address)
        .unwrap()
        .raw_text()
        .to_owned()
}

fn display(state: &AppState, address: CellAddress) -> String {
    state
        .sheets_api
        .as_ref()
        .unwrap()
        .cell_view(address.sheet, address)
        .unwrap()
        .display_text()
        .to_owned()
}

fn format(state: &AppState, address: CellAddress) -> Option<sheets_core::CellFormat> {
    state
        .sheets_api
        .as_ref()
        .unwrap()
        .cell_view(address.sheet, address)
        .unwrap()
        .format
}

fn write(state: &mut AppState, row: u32, column: u32, value: &str) {
    let address = address(state, row, column);
    state.sheets_select_cell(address);
    state.apply_sheets_cell_input(value.to_owned()).unwrap();
}

fn select_range(state: &mut AppState, start: (u32, u32), end: (u32, u32)) {
    let start = address(state, start.0, start.1);
    let end = address(state, end.0, end.1);
    state
        .sheets_view
        .as_mut()
        .unwrap()
        .select_range(start, end)
        .unwrap();
}

fn set_sort_draft(state: &mut AppState, column: &str, has_header: bool) {
    state.update_sheets_data_draft(Some(column.to_owned()), Some(has_header), None, None);
}

fn set_equals_filter_draft(state: &mut AppState, column: &str, value: &str) {
    state.update_sheets_data_draft(
        Some(column.to_owned()),
        Some(true),
        Some(0),
        Some(value.to_owned()),
    );
}

fn fill_group_table(state: &mut AppState) -> SheetId {
    for (row, (group, quantity)) in [
        ("Group", "Qty"),
        ("A", "10"),
        ("B", "5"),
        ("A", "7"),
        ("C", "1"),
        ("B", "12"),
    ]
    .into_iter()
    .enumerate()
    {
        write(state, row as u32, 0, group);
        write(state, row as u32, 1, quantity);
    }
    active_sheet(state)
}

#[test]
fn sort_and_filter_budget_rejections_are_atomic_and_explained_in_russian() {
    let mut state = AppState::default();
    let id = add_book(&mut state, "history-budget.xlsx");
    state.activate_document(id).unwrap();
    write(&mut state, 0, 0, "2");
    write(&mut state, 1, 0, "1");
    select_range(&mut state, (0, 0), (1, 0));
    set_sort_draft(&mut state, "A", false);
    state
        .sheets_api
        .as_mut()
        .unwrap()
        .set_history_limits(sheets_api::HistoryLimits {
            max_retained_cells: 2,
            ..Default::default()
        });
    let before = state.sheets_api.as_ref().unwrap().document().snapshot();
    let history = state.sheets_api.as_ref().unwrap().history_stats();
    let error = state
        .apply_ribbon_command(RibbonCommand::SortAscending)
        .unwrap_err();
    assert!(error.contains("превышен бюджет отмены"), "{error}");
    assert!(error.contains("не изменены"));
    assert_eq!(
        state.sheets_api.as_ref().unwrap().document().snapshot(),
        before
    );
    assert_eq!(state.sheets_api.as_ref().unwrap().history_stats(), history);

    state
        .sheets_api
        .as_mut()
        .unwrap()
        .set_history_limits(sheets_api::HistoryLimits {
            max_retained_bytes: 1,
            ..Default::default()
        });
    set_equals_filter_draft(&mut state, "A", "1");
    let history = state.sheets_api.as_ref().unwrap().history_stats();
    let error = state
        .apply_ribbon_command(RibbonCommand::ApplyAutoFilter)
        .unwrap_err();
    assert!(error.contains("превышен бюджет отмены"), "{error}");
    assert_eq!(
        state.sheets_api.as_ref().unwrap().document().snapshot(),
        before
    );
    assert_eq!(state.sheets_api.as_ref().unwrap().history_stats(), history);
}

#[test]
fn sort_rejects_single_cells_merged_ranges_and_source_backed_books_without_mutation() {
    let mut state = AppState::default();
    let id = add_book(&mut state, "sort-rejections.xlsx");
    state.activate_document(id).unwrap();
    write(&mut state, 0, 0, "kept");
    select_range(&mut state, (0, 0), (1, 1));
    state
        .apply_ribbon_command(RibbonCommand::MergeSelection)
        .unwrap();

    let sheet = active_sheet(&state);
    let before_snapshot = state.sheets_snapshot().unwrap().clone();
    let before_history = state.sheets_api.as_ref().unwrap().history_stats();
    set_sort_draft(&mut state, "A", false);
    select_range(&mut state, (0, 0), (1, 1));
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::SortAscending)
            .is_err()
    );
    assert_eq!(state.sheets_snapshot().unwrap(), &before_snapshot);
    assert_eq!(
        state.sheets_api.as_ref().unwrap().history_stats(),
        before_history
    );

    select_range(&mut state, (4, 2), (4, 2));
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::SortAscending)
            .is_err()
    );
    assert_eq!(state.sheets_snapshot().unwrap(), &before_snapshot);
    assert_eq!(
        state.sheets_api.as_ref().unwrap().history_stats(),
        before_history
    );
    assert_eq!(active_sheet(&state), sheet);

    let source_api = SpreadsheetApi::new(IroncalcAdapter::new());
    let source_bytes: Arc<[u8]> = Arc::from(source_api.export_xlsx().unwrap());
    let initial =
        import_bounded_sheet_window(&source_bytes, BoundedSheetWindow::initial()).unwrap();
    let read_only = state
        .register_document_with_source(
            DocumentSource::from_path_with_retained_bytes(
                PathBuf::from("read-only-sort-filter.xlsx"),
                source_bytes,
            ),
            OpenDocument::Sheets(initial),
        )
        .unwrap();
    state.activate_document(read_only).unwrap();
    let read_only_snapshot = state.sheets_snapshot().unwrap().clone();
    assert!(!state.ribbon_command_available(RibbonCommand::SortAscending));
    assert!(!state.ribbon_command_available(RibbonCommand::ApplyAutoFilter));
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::SortAscending)
            .is_err()
    );
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::ApplyAutoFilter)
            .is_err()
    );
    assert_eq!(state.sheets_snapshot().unwrap(), &read_only_snapshot);
}

#[test]
fn sort_keeps_header_formulas_and_formats_with_rows_in_one_undoable_entry() {
    let mut state = AppState::default();
    let id = add_book(&mut state, "formula-sort.xlsx");
    state.activate_document(id).unwrap();
    for (column, title) in ["Item", "Qty", "Price", "Total"].into_iter().enumerate() {
        write(&mut state, 0, column as u32, title);
    }
    for (row, values) in [
        ["Apple", "3", "10", "=B2*C2"],
        ["Pear", "1", "20", "=B3*C3"],
        ["Plum", "2", "15", "=B4*C4"],
    ]
    .into_iter()
    .enumerate()
    {
        for (column, value) in values.into_iter().enumerate() {
            write(&mut state, row as u32 + 1, column as u32, value);
        }
    }

    let sheet = active_sheet(&state);
    let formatted_cell = address(&state, 1, 1);
    let moved_format = sheets_core::CellFormat {
        background: Some(sheets_core::Color::new(255, 255, 0, 255)),
        ..Default::default()
    };
    state
        .sheets_api
        .as_mut()
        .unwrap()
        .set_cell_format(
            sheet,
            formatted_cell,
            moved_format.clone(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
    state.commit_sheets_api_snapshot().unwrap();

    let entries_before_sort = state
        .sheets_api
        .as_ref()
        .unwrap()
        .history_stats()
        .undo_entries;
    select_range(&mut state, (0, 0), (3, 3));
    set_sort_draft(&mut state, "B", true);
    state
        .apply_ribbon_command(RibbonCommand::SortAscending)
        .unwrap();
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .history_stats()
            .undo_entries,
        entries_before_sort + 1
    );

    for (column, title) in ["Item", "Qty", "Price", "Total"].into_iter().enumerate() {
        assert_eq!(raw(&state, address(&state, 0, column as u32)), title);
    }
    assert_eq!(raw(&state, address(&state, 1, 0)), "Pear");
    assert_eq!(raw(&state, address(&state, 2, 0)), "Plum");
    assert_eq!(raw(&state, address(&state, 3, 0)), "Apple");
    assert_eq!(raw(&state, address(&state, 1, 3)), "=B2*C2");
    assert_eq!(raw(&state, address(&state, 2, 3)), "=B3*C3");
    assert_eq!(raw(&state, address(&state, 3, 3)), "=B4*C4");
    assert_eq!(display(&state, address(&state, 1, 3)), "20");
    assert_eq!(display(&state, address(&state, 2, 3)), "30");
    assert_eq!(display(&state, address(&state, 3, 3)), "30");
    assert_eq!(
        format(&state, address(&state, 3, 1)),
        Some(moved_format.clone())
    );

    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .history_stats()
            .undo_entries,
        entries_before_sort
    );
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .history_stats()
            .redo_entries,
        1
    );
    assert_eq!(raw(&state, address(&state, 1, 0)), "Apple");
    assert_eq!(raw(&state, address(&state, 2, 0)), "Pear");
    assert_eq!(raw(&state, address(&state, 3, 0)), "Plum");
    assert_eq!(
        format(&state, address(&state, 1, 1)),
        Some(moved_format.clone())
    );
    assert_eq!(format(&state, address(&state, 3, 1)), None);

    state
        .dispatch_history_command(id, OfficeHistoryCommand::Redo)
        .unwrap();
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .history_stats()
            .undo_entries,
        entries_before_sort + 1
    );
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .history_stats()
            .redo_entries,
        0
    );
    assert_eq!(raw(&state, address(&state, 1, 0)), "Pear");
    assert_eq!(raw(&state, address(&state, 3, 0)), "Apple");
    assert_eq!(format(&state, address(&state, 3, 1)), Some(moved_format));
}

#[test]
fn filter_hides_rows_from_frame_and_navigation_and_clear_is_undoable() {
    let mut state = AppState::default();
    let id = add_book(&mut state, "filter-frame.xlsx");
    state.activate_document(id).unwrap();
    let sheet = fill_group_table(&mut state);
    select_range(&mut state, (0, 0), (5, 1));
    set_equals_filter_draft(&mut state, "A", "A");
    state
        .apply_ribbon_command(RibbonCommand::ApplyAutoFilter)
        .unwrap();

    let hidden = [RowIndex(2), RowIndex(4), RowIndex(5)];
    assert_eq!(
        state.sheets_api.as_ref().unwrap().filter_hidden_rows(sheet),
        hidden
    );
    let selected_range = CellRange::new(address(&state, 0, 0), address(&state, 5, 1)).unwrap();
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection().range(),
        Some(selected_range)
    );
    let frame = state.sheets_frame().unwrap();
    for row in hidden {
        assert!(frame.row_headers.iter().all(|header| header.index != row.0));
        assert!(frame.cells.iter().all(|cell| cell.address.row != row));
    }

    let selection_before_hidden_goto = state.sheets_view.as_ref().unwrap().selection().clone();
    state.update_sheets_tools_draft(Some("A3".into()), None);
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::GoToRange)
            .is_err()
    );
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection(),
        &selection_before_hidden_goto
    );

    state.update_sheets_tools_draft(None, Some("A".into()));
    state.apply_ribbon_command(RibbonCommand::FindNext).unwrap();
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection().focus,
        Some(address(&state, 1, 0))
    );
    state.apply_ribbon_command(RibbonCommand::FindNext).unwrap();
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection().focus,
        Some(address(&state, 3, 0))
    );
    state.update_sheets_tools_draft(None, Some("B".into()));
    assert!(state.apply_ribbon_command(RibbonCommand::FindNext).is_err());

    state
        .apply_ribbon_command(RibbonCommand::ClearAutoFilter)
        .unwrap();
    assert!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .filter_hidden_rows(sheet)
            .is_empty()
    );
    assert!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .auto_filter(sheet)
            .is_none()
    );
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(
        state.sheets_api.as_ref().unwrap().filter_hidden_rows(sheet),
        hidden
    );
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Redo)
        .unwrap();
    assert!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .filter_hidden_rows(sheet)
            .is_empty()
    );
}

#[test]
fn filter_applied_after_sort_tracks_sorted_rows_through_undo_and_redo() {
    let mut state = AppState::default();
    let id = add_book(&mut state, "sort-then-filter.xlsx");
    state.activate_document(id).unwrap();
    let sheet = fill_group_table(&mut state);

    select_range(&mut state, (0, 0), (5, 1));
    set_sort_draft(&mut state, "B", true);
    state
        .apply_ribbon_command(RibbonCommand::SortAscending)
        .unwrap();
    assert_eq!(raw(&state, address(&state, 1, 0)), "C");
    assert_eq!(raw(&state, address(&state, 2, 0)), "B");
    assert_eq!(raw(&state, address(&state, 3, 0)), "A");
    assert_eq!(raw(&state, address(&state, 4, 0)), "A");
    assert_eq!(raw(&state, address(&state, 5, 0)), "B");

    select_range(&mut state, (0, 0), (5, 1));
    set_equals_filter_draft(&mut state, "A", "A");
    state
        .apply_ribbon_command(RibbonCommand::ApplyAutoFilter)
        .unwrap();
    let filtered_rows = [RowIndex(1), RowIndex(2), RowIndex(5)];
    assert_eq!(
        state.sheets_api.as_ref().unwrap().filter_hidden_rows(sheet),
        filtered_rows
    );

    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .auto_filter(sheet)
            .is_none()
    );
    assert!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .filter_hidden_rows(sheet)
            .is_empty()
    );
    assert_eq!(raw(&state, address(&state, 1, 0)), "C");
    assert_eq!(raw(&state, address(&state, 5, 0)), "B");

    state
        .dispatch_history_command(id, OfficeHistoryCommand::Redo)
        .unwrap();
    assert_eq!(
        state.sheets_api.as_ref().unwrap().filter_hidden_rows(sheet),
        filtered_rows
    );
}

#[test]
fn sheet_switch_routes_undo_only_to_the_active_sheets_sort_or_filter() {
    let mut state = AppState::default();
    let id = add_book(&mut state, "sheet-history.xlsx");
    state.activate_document(id).unwrap();
    state.apply_ribbon_command(RibbonCommand::AddSheet).unwrap();

    state.select_sheet_index(0);
    let sheet_one = active_sheet(&state);
    write(&mut state, 0, 0, "Value");
    write(&mut state, 1, 0, "z");
    write(&mut state, 2, 0, "a");
    select_range(&mut state, (0, 0), (2, 0));
    set_sort_draft(&mut state, "A", true);
    state
        .apply_ribbon_command(RibbonCommand::SortAscending)
        .unwrap();
    assert_eq!(raw(&state, address(&state, 1, 0)), "a");

    state.select_sheet_index(1);
    let sheet_two = active_sheet(&state);
    assert_ne!(sheet_one, sheet_two);
    write(&mut state, 0, 0, "Group");
    write(&mut state, 1, 0, "A");
    write(&mut state, 2, 0, "B");
    select_range(&mut state, (0, 0), (2, 0));
    set_equals_filter_draft(&mut state, "A", "A");
    state
        .apply_ribbon_command(RibbonCommand::ApplyAutoFilter)
        .unwrap();
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .filter_hidden_rows(sheet_two),
        [RowIndex(2)]
    );

    state.select_sheet_index(0);
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, address(&state, 1, 0)), "z");
    assert_eq!(raw(&state, address(&state, 2, 0)), "a");
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .filter_hidden_rows(sheet_two),
        [RowIndex(2)]
    );

    state.select_sheet_index(1);
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .filter_hidden_rows(sheet_two)
            .is_empty()
    );
    assert!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .auto_filter(sheet_two)
            .is_none()
    );
    state.select_sheet_index(0);
    assert_eq!(raw(&state, address(&state, 1, 0)), "z");
}

#[test]
fn document_activation_and_close_keep_sort_filter_apis_and_histories_independent() {
    let mut state = AppState::default();
    let first = add_book(&mut state, "first-sort.xlsx");
    let second = add_book(&mut state, "second-filter.xlsx");

    state.activate_document(first).unwrap();
    let first_sheet = fill_group_table(&mut state);
    select_range(&mut state, (0, 0), (5, 1));
    set_sort_draft(&mut state, "B", true);
    state
        .apply_ribbon_command(RibbonCommand::SortAscending)
        .unwrap();
    assert_eq!(raw(&state, address(&state, 1, 0)), "C");
    let first_token = state.sheets_api.as_ref().unwrap().document().token();

    state.activate_document(second).unwrap();
    let second_sheet = fill_group_table(&mut state);
    select_range(&mut state, (0, 0), (5, 1));
    set_equals_filter_draft(&mut state, "A", "A");
    state
        .apply_ribbon_command(RibbonCommand::ApplyAutoFilter)
        .unwrap();
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .filter_hidden_rows(second_sheet),
        [RowIndex(2), RowIndex(4), RowIndex(5)]
    );

    state.activate_document(first).unwrap();
    assert_eq!(
        state.sheets_api.as_ref().unwrap().document().token(),
        first_token
    );
    state
        .dispatch_history_command(first, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, address(&state, 1, 0)), "A");

    state.activate_document(second).unwrap();
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .filter_hidden_rows(second_sheet),
        [RowIndex(2), RowIndex(4), RowIndex(5)]
    );
    state
        .dispatch_history_command(second, OfficeHistoryCommand::Undo)
        .unwrap();
    assert!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .filter_hidden_rows(second_sheet)
            .is_empty()
    );

    state.activate_document(first).unwrap();
    assert_eq!(raw(&state, address(&state, 1, 0)), "A");
    state.close_document(second).unwrap();
    assert!(state.documents.get(second).is_none());
    assert_eq!(state.active_document_id(), Some(first));
    state
        .dispatch_history_command(first, OfficeHistoryCommand::Redo)
        .unwrap();
    assert_eq!(raw(&state, address(&state, 1, 0)), "C");
    assert_eq!(active_sheet(&state), first_sheet);
}
