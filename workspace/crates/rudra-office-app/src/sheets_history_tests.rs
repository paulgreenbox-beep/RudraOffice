//! RC25 application-boundary regressions; canonical state lives in SheetsUi/API.
use super::*;
use sheets_grid::Point;

fn add_book(state: &mut AppState, name: &str) -> RuntimeDocumentId {
    let api = SpreadsheetApi::new(IroncalcAdapter::new());
    state
        .register_document(
            PathBuf::from(name),
            OpenDocument::Sheets(api.document().snapshot()),
        )
        .unwrap()
}

fn address(state: &AppState, row: u32, column: u32) -> CellAddress {
    CellAddress::new(
        state.sheets_api.as_ref().unwrap().document().active_sheet(),
        RowIndex(row),
        ColumnIndex(column),
    )
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

fn cell_center(state: &AppState, row: u32, column: u32) -> Point {
    let address = address(state, row, column);
    let frame = state.sheets_frame().unwrap();
    let rect = frame
        .cells
        .iter()
        .find(|cell| cell.address == address)
        .unwrap()
        .rect;
    Point {
        x: rect.x + rect.w / 2.0,
        y: rect.y + rect.h / 2.0,
    }
}

fn drag(state: &mut AppState, from: (u32, u32), to: (u32, u32)) {
    let start = cell_center(state, from.0, from.1);
    let end = cell_center(state, to.0, to.1);
    state.sheets_handle_selection_input(UiEvent::PointerDown {
        point: start,
        button: sheets_ui::PointerButton::Primary,
        modifiers: Default::default(),
    });
    state.sheets_handle_selection_input(UiEvent::PointerMove {
        point: end,
        modifiers: Default::default(),
    });
    state.sheets_handle_selection_input(UiEvent::PointerUp {
        point: end,
        button: sheets_ui::PointerButton::Primary,
        modifiers: Default::default(),
    });
}

#[test]
fn mouse_range_clear_undo_redo_recalculates_canonical_formulas() {
    let mut state = AppState::default();
    let id = add_book(&mut state, "undo.xlsx");
    state.activate_document(id).unwrap();
    let a1 = address(&state, 0, 0);
    let b1 = address(&state, 0, 1);
    for (cell, input) in [(a1, "10"), (b1, "=A1*2")] {
        state.sheets_select_cell(cell);
        state.apply_sheets_cell_input(input.into()).unwrap();
    }
    let before = state.sheets_api.as_ref().unwrap().history_stats();
    drag(&mut state, (0, 0), (2, 2));
    assert_eq!(
        state
            .sheets_view
            .as_ref()
            .unwrap()
            .selection()
            .range()
            .unwrap(),
        CellRange::new(a1, address(&state, 2, 2)).unwrap()
    );
    assert_eq!(state.sheets_api.as_ref().unwrap().history_stats(), before);
    state
        .apply_ribbon_command(RibbonCommand::ClearRange)
        .unwrap();
    assert_eq!(raw(&state, a1), "");
    assert_eq!(raw(&state, b1), "");
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, a1), "10");
    assert_eq!(raw(&state, b1), "=A1*2");
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .cell_view(b1.sheet, b1)
            .unwrap()
            .display_text(),
        "20"
    );
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Redo)
        .unwrap();
    assert_eq!(raw(&state, b1), "");
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    state.sheets_select_cell(a1);
    state.apply_sheets_cell_input("7".into()).unwrap();
    assert!(!state.sheets_can_redo());
    assert_eq!(
        state
            .sheets_api
            .as_ref()
            .unwrap()
            .cell_view(b1.sheet, b1)
            .unwrap()
            .display_text(),
        "14"
    );
}

#[test]
fn document_switch_keeps_api_token_selection_and_independent_histories() {
    let mut state = AppState::default();
    let first = add_book(&mut state, "a.xlsx");
    let second = add_book(&mut state, "b.xlsx");
    state.activate_document(first).unwrap();
    let a1 = address(&state, 0, 0);
    state.sheets_select_cell(a1);
    state.apply_sheets_cell_input("Альфа".into()).unwrap();
    drag(&mut state, (2, 2), (0, 0));
    let first_selection = state.sheets_view.as_ref().unwrap().selection().clone();
    let token = state.sheets_api.as_ref().unwrap().document().token();
    state.activate_document(second).unwrap();
    let b2 = address(&state, 1, 1);
    state.sheets_select_cell(b2);
    state.apply_sheets_cell_input("Бета".into()).unwrap();
    assert!(
        state
            .dispatch_history_command(first, OfficeHistoryCommand::Undo)
            .is_err()
    );
    state
        .dispatch_history_command(second, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, b2), "");
    state.activate_document(first).unwrap();
    assert_eq!(state.sheets_api.as_ref().unwrap().document().token(), token);
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection(),
        &first_selection
    );
    assert_eq!(raw(&state, a1), "Альфа");
    state
        .dispatch_history_command(first, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, a1), "");
    state.activate_document(second).unwrap();
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection().focus,
        Some(b2)
    );
    assert!(state.sheets_can_redo());
    state
        .dispatch_history_command(second, OfficeHistoryCommand::Redo)
        .unwrap();
    assert_eq!(raw(&state, b2), "Бета");
    state.activate_document(first).unwrap();
    assert_eq!(raw(&state, a1), "");
}

#[test]
fn sheet_switch_restores_selection_without_creating_history() {
    let mut state = AppState::default();
    let id = add_book(&mut state, "sheets.xlsx");
    state.activate_document(id).unwrap();
    state.apply_ribbon_command(RibbonCommand::AddSheet).unwrap();
    state.select_sheet_index(0);
    drag(&mut state, (0, 0), (2, 2));
    let first = state.sheets_view.as_ref().unwrap().selection().clone();
    state.select_sheet_index(1);
    drag(&mut state, (1, 1), (3, 3));
    let second = state.sheets_view.as_ref().unwrap().selection().clone();
    let stats = state.sheets_api.as_ref().unwrap().history_stats();
    state.select_sheet_index(0);
    assert_eq!(state.sheets_view.as_ref().unwrap().selection(), &first);
    state.select_sheet_index(1);
    assert_eq!(state.sheets_view.as_ref().unwrap().selection(), &second);
    assert_eq!(state.sheets_api.as_ref().unwrap().history_stats(), stats);
}

#[test]
fn merge_unmerge_rejection_noop_and_history_share_one_transaction_path() {
    let mut state = AppState::default();
    let id = add_book(&mut state, "merge.xlsx");
    state.activate_document(id).unwrap();
    let b1 = address(&state, 0, 1);
    state.sheets_select_cell(b1);
    state.apply_sheets_cell_input("Сохранить".into()).unwrap();
    drag(&mut state, (0, 0), (1, 1));
    let stats = state.sheets_api.as_ref().unwrap().history_stats();
    assert!(
        state
            .apply_ribbon_command(RibbonCommand::MergeSelection)
            .is_err()
    );
    assert_eq!(state.sheets_api.as_ref().unwrap().history_stats(), stats);
    state.sheets_select_cell(b1);
    state.apply_sheets_cell_input(String::new()).unwrap();
    drag(&mut state, (0, 0), (1, 1));
    state
        .apply_ribbon_command(RibbonCommand::MergeSelection)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets[0].merges.len(), 1);
    let merged = state.sheets_api.as_ref().unwrap().history_stats();
    state
        .apply_ribbon_command(RibbonCommand::MergeSelection)
        .unwrap();
    assert_eq!(state.sheets_api.as_ref().unwrap().history_stats(), merged);
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert!(state.sheets_snapshot().unwrap().sheets[0].merges.is_empty());
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Redo)
        .unwrap();
    state
        .apply_ribbon_command(RibbonCommand::UnmergeSelection)
        .unwrap();
    assert!(state.sheets_snapshot().unwrap().sheets[0].merges.is_empty());
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_snapshot().unwrap().sheets[0].merges.len(), 1);
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Redo)
        .unwrap();
    assert!(state.sheets_snapshot().unwrap().sheets[0].merges.is_empty());
}

#[test]
fn shift_navigation_preserves_anchor_and_no_history_at_app_boundary() {
    let mut state = AppState::default();
    let id = add_book(&mut state, "shift.xlsx");
    state.activate_document(id).unwrap();
    let b2 = address(&state, 1, 1);
    state.sheets_select_cell(b2);
    let shift = KeyModifiers {
        shift: true,
        ..Default::default()
    };
    state.sheets_handle_key(KeyCode::ArrowRight, shift);
    state.sheets_handle_key(KeyCode::ArrowDown, shift);
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection().anchor,
        Some(b2)
    );
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection().focus,
        Some(address(&state, 2, 2))
    );
    for key in [
        KeyCode::ArrowLeft,
        KeyCode::ArrowLeft,
        KeyCode::ArrowUp,
        KeyCode::ArrowUp,
    ] {
        state.sheets_handle_key(key, shift);
    }
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection().anchor,
        Some(b2)
    );
    assert_eq!(
        state.sheets_view.as_ref().unwrap().selection().focus,
        Some(address(&state, 0, 0))
    );
    assert!(!state.sheets_can_undo());
}

#[test]
fn closing_a_cached_session_drops_its_history_and_selection() {
    let mut state = AppState::default();
    let first = add_book(&mut state, "closed.xlsx");
    let second = add_book(&mut state, "remaining.xlsx");
    state.activate_document(first).unwrap();
    state.sheets_select_cell(address(&state, 0, 0));
    state.apply_sheets_cell_input("42".into()).unwrap();
    state.activate_document(second).unwrap();
    assert!(
        state
            .documents
            .require(first)
            .unwrap()
            .sheets_api
            .as_ref()
            .unwrap()
            .can_undo()
    );
    state.close_document(first).unwrap();
    assert!(state.documents.get(first).is_none());
    assert!(
        state
            .dispatch_history_command(first, OfficeHistoryCommand::Undo)
            .is_err()
    );
    assert!(!state.sheets_can_undo());
}

#[test]
fn activation_failure_does_not_discard_the_previously_parked_history() {
    let mut state = AppState::default();
    let good = add_book(&mut state, "good.xlsx");
    state.activate_document(good).unwrap();
    state.sheets_select_cell(address(&state, 0, 0));
    state.apply_sheets_cell_input("42".into()).unwrap();
    let token = state.sheets_api.as_ref().unwrap().document().token();
    let bad = state
        .register_document(
            PathBuf::from("invalid.xlsx"),
            OpenDocument::Sheets(WorkbookSnapshot::default()),
        )
        .unwrap();
    assert!(state.activate_document(bad).is_err());
    // This is the same recovery used by App::activate_document.
    state.activate_document(good).unwrap();
    assert_eq!(state.sheets_api.as_ref().unwrap().document().token(), token);
    assert!(state.sheets_can_undo());
    assert_eq!(state.sheets_selected_raw_text().as_deref(), Some("42"));
    state
        .dispatch_history_command(good, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(state.sheets_selected_raw_text().as_deref(), Some(""));
}
