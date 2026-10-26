//! RC28 app ownership/identity regressions; no actual desktop clipboard in unit tests.
use super::*;

fn book(state: &mut AppState, name: &str) -> RuntimeDocumentId {
    let api = SpreadsheetApi::production();
    let id = state
        .register_document(name.into(), OpenDocument::Sheets(api.document().snapshot()))
        .unwrap();
    state.activate_document(id).unwrap();
    id
}

fn address(state: &AppState, row: u32, col: u32) -> CellAddress {
    CellAddress::new(
        state.sheets_api.as_ref().unwrap().document().active_sheet(),
        RowIndex(row),
        ColumnIndex(col),
    )
}

fn select(state: &mut AppState, a: (u32, u32), b: (u32, u32)) {
    let start = address(state, a.0, a.1);
    let end = address(state, b.0, b.1);
    state
        .sheets_view
        .as_mut()
        .unwrap()
        .select_range(start, end)
        .unwrap();
}

fn write(state: &mut AppState, row: u32, col: u32, input: &str) {
    select(state, (row, col), (row, col));
    state.apply_sheets_cell_input(input.into()).unwrap();
}

fn raw(state: &AppState, row: u32, col: u32) -> String {
    let cell = address(state, row, col);
    state
        .sheets_api
        .as_ref()
        .unwrap()
        .cell_view(cell.sheet, cell)
        .unwrap()
        .raw_text()
}

fn copy(state: &mut AppState, cut: bool) -> windows_clipboard::ClipboardObservation {
    let payload = state.capture_sheets_clipboard(cut).unwrap();
    let text = payload.tsv().to_owned();
    state
        .claim_sheets_clipboard(
            payload,
            cut,
            windows_clipboard::ClipboardObservation {
                text: text.clone(),
                sequence: 77,
            },
        )
        .unwrap();
    windows_clipboard::ClipboardObservation { text, sequence: 77 }
}

#[test]
fn copy_preserves_revision_history_and_redo_then_uses_snapshot_after_source_edit() {
    let mut state = AppState::default();
    book(&mut state, "copy.xlsx");
    write(&mut state, 0, 0, "Русский 😀");
    write(&mut state, 0, 1, "10");
    let id = state.active_document_id().unwrap();
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    select(&mut state, (0, 0), (0, 0));
    let api = state.sheets_api.as_ref().unwrap();
    let revision = api.document().revision();
    let history = api.history_stats();
    let observation = copy(&mut state, false);
    assert_eq!(
        state.sheets_api.as_ref().unwrap().document().revision(),
        revision
    );
    assert_eq!(state.sheets_api.as_ref().unwrap().history_stats(), history);
    assert!(state.sheets_can_redo());
    write(&mut state, 0, 0, "изменённый источник");
    select(&mut state, (2, 2), (2, 2));
    state.paste_sheets_clipboard(observation).unwrap();
    assert_eq!(raw(&state, 2, 2), "Русский 😀");
}

#[test]
fn identical_external_text_with_new_sequence_pastes_values_not_stale_formulas() {
    let mut state = AppState::default();
    book(&mut state, "external.xlsx");
    write(&mut state, 0, 0, "10");
    write(&mut state, 0, 1, "=A1*2");
    let mut observation = copy(&mut state, false);
    assert_eq!(observation.text.trim_end(), "20");
    observation.sequence += 1;
    select(&mut state, (1, 1), (1, 1));
    state.paste_sheets_clipboard(observation).unwrap();
    assert_eq!(raw(&state, 1, 1), "20");
}

#[test]
fn native_copy_formula_relocates_and_paste_is_one_undo_redo_transaction() {
    let mut state = AppState::default();
    book(&mut state, "formula.xlsx");
    write(&mut state, 0, 0, "10");
    write(&mut state, 0, 1, "=A1+$A$1");
    select(&mut state, (0, 0), (0, 1));
    let observation = copy(&mut state, false);
    select(&mut state, (2, 2), (2, 2));
    let before = state.sheets_api.as_ref().unwrap().history_stats();
    state.paste_sheets_clipboard(observation).unwrap();
    assert_eq!(raw(&state, 2, 2), "10");
    assert_eq!(raw(&state, 2, 3), "=C3+$A$1");
    let after = state.sheets_api.as_ref().unwrap().history_stats();
    assert_eq!(after.undo_entries, before.undo_entries + 1);
    let id = state.active_document_id().unwrap();
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, 2, 2), "");
    assert_eq!(raw(&state, 2, 3), "");
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Redo)
        .unwrap();
    assert_eq!(raw(&state, 2, 3), "=C3+$A$1");
}

#[test]
fn cut_does_not_clear_until_paste_and_mutation_or_close_invalidates_it() {
    let mut state = AppState::default();
    let id = book(&mut state, "cut.xlsx");
    write(&mut state, 0, 0, "источник");
    let observation = copy(&mut state, true);
    assert_eq!(raw(&state, 0, 0), "источник");
    select(&mut state, (1, 1), (1, 1));
    state.paste_sheets_clipboard(observation).unwrap();
    assert_eq!(raw(&state, 0, 0), "");
    assert_eq!(raw(&state, 1, 1), "источник");
    assert!(state.sheets_clipboard.pending().is_none());
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, 0, 0), "источник");
    assert_eq!(raw(&state, 1, 1), "");
    select(&mut state, (0, 0), (0, 0));
    copy(&mut state, true);
    write(&mut state, 3, 3, "другая мутация");
    assert!(state.sheets_clipboard.pending().is_none());
    select(&mut state, (0, 0), (0, 0));
    copy(&mut state, true);
    state.close_document(id).unwrap();
    assert!(state.sheets_clipboard.pending().is_none());
}

#[test]
fn copy_crosses_typed_documents_and_survives_source_close_but_cut_does_not() {
    let mut state = AppState::default();
    let source = book(&mut state, "same-name.xlsx");
    write(&mut state, 0, 0, "первая книга");
    let observation = copy(&mut state, false);
    let destination = book(&mut state, "other/same-name.xlsx");
    assert_ne!(source, destination);
    state.close_document(source).unwrap();
    select(&mut state, (1, 1), (1, 1));
    state.paste_sheets_clipboard(observation).unwrap();
    assert_eq!(raw(&state, 1, 1), "первая книга");
    select(&mut state, (1, 1), (1, 1));
    let cut = copy(&mut state, true);
    book(&mut state, "third.xlsx");
    select(&mut state, (0, 0), (0, 0));
    let revision = state.sheets_api.as_ref().unwrap().document().revision();
    assert!(
        state
            .paste_sheets_clipboard(cut)
            .unwrap_err()
            .contains("разными книгами")
    );
    assert_eq!(
        state.sheets_api.as_ref().unwrap().document().revision(),
        revision
    );
    assert!(state.sheets_clipboard.pending().is_some());
}

#[test]
fn same_book_cross_sheet_cut_survives_navigation_and_can_undo_as_one_operation() {
    let mut state = AppState::default();
    let id = book(&mut state, "sheets.xlsx");
    state.apply_ribbon_command(RibbonCommand::AddSheet).unwrap();
    state.select_sheet_index(0);
    write(&mut state, 0, 0, "между листами");
    let observation = copy(&mut state, true);
    state.select_sheet_index(1);
    select(&mut state, (1, 1), (1, 1));
    state.paste_sheets_clipboard(observation).unwrap();
    assert_eq!(raw(&state, 1, 1), "между листами");
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, 1, 1), "");
    state.select_sheet_index(0);
    assert_eq!(raw(&state, 0, 0), "между листами");
}

#[test]
fn failed_shape_and_partial_merge_paste_are_atomic() {
    let mut state = AppState::default();
    book(&mut state, "atomic.xlsx");
    write(&mut state, 0, 0, "A");
    write(&mut state, 0, 1, "B");
    select(&mut state, (0, 0), (0, 1));
    let observation = copy(&mut state, false);
    select(&mut state, (2, 2), (3, 3));
    let before = state.sheets_api.as_ref().unwrap().document().snapshot();
    assert!(state.paste_sheets_clipboard(observation).is_err());
    assert_eq!(
        state.sheets_api.as_ref().unwrap().document().snapshot(),
        before
    );
}

#[test]
fn oversized_copy_rejects_without_destroying_previous_claim() {
    let mut state = AppState::default();
    book(&mut state, "bounded.xlsx");
    write(&mut state, 0, 0, "saved");
    let observation = copy(&mut state, false);
    select(&mut state, (0, 0), (100, 100));
    assert!(state.capture_sheets_clipboard(false).is_err());
    assert!(
        state
            .sheets_clipboard
            .matching(observation.sequence, &observation.text)
            .is_some()
    );
}

#[test]
fn paste_into_filtered_row_succeeds_and_history_recomputes_visibility() {
    let mut state = AppState::default();
    let id = book(&mut state, "filter-paste.xlsx");
    for (row, status) in ["Status", "Active", "Inactive", "Active"]
        .into_iter()
        .enumerate()
    {
        write(&mut state, row as u32, 0, status);
    }
    select(&mut state, (0, 0), (3, 0));
    state.update_sheets_data_draft(Some("A".into()), Some(true), Some(0), Some("Active".into()));
    state.filter_sheets_selection().unwrap();
    let sheet = state.sheets_api.as_ref().unwrap().document().active_sheet();
    assert_eq!(
        state.sheets_api.as_ref().unwrap().filter_hidden_rows(sheet),
        vec![RowIndex(2)]
    );
    select(&mut state, (1, 0), (1, 0));
    state
        .paste_sheets_clipboard(windows_clipboard::ClipboardObservation {
            text: "Inactive".into(),
            sequence: 123,
        })
        .unwrap();
    assert_eq!(raw(&state, 1, 0), "Inactive");
    assert_eq!(
        state.sheets_api.as_ref().unwrap().filter_hidden_rows(sheet),
        vec![RowIndex(1), RowIndex(2)]
    );
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Undo)
        .unwrap();
    assert_eq!(raw(&state, 1, 0), "Active");
    assert_eq!(
        state.sheets_api.as_ref().unwrap().filter_hidden_rows(sheet),
        vec![RowIndex(2)]
    );
    state
        .dispatch_history_command(id, OfficeHistoryCommand::Redo)
        .unwrap();
    assert_eq!(
        state.sheets_api.as_ref().unwrap().filter_hidden_rows(sheet),
        vec![RowIndex(1), RowIndex(2)]
    );
}

#[test]
fn source_backed_copy_rejects_unverified_values_topology_inside_resident_window() {
    let mut state = AppState::default();
    let api = SpreadsheetApi::production();
    let session = DocumentSession::new(
        DocumentSource::from_path_with_retained_bytes("large.xlsx".into(), Arc::from([])),
        OpenDocument::Sheets(api.document().snapshot()),
    )
    .unwrap();
    let id = state.documents.insert(session).unwrap();
    state.active_document = Some(id);
    state.bind_sheets_api(api).unwrap();
    state.sheets_bounded_window = Some(BoundedSheetWindow::new(0, 0, 0, 4, 4, 16));
    select(&mut state, (0, 0), (2, 2));
    assert!(state.capture_sheets_clipboard(false).is_err());
    assert!(state.capture_sheets_clipboard(true).is_err());
    assert!(
        state
            .paste_sheets_clipboard(windows_clipboard::ClipboardObservation {
                text: "no mutation".into(),
                sequence: 1
            })
            .is_err()
    );
    select(&mut state, (0, 0), (4, 4));
    assert!(state.capture_sheets_clipboard(false).is_err());
}

#[test]
fn real_bounded_import_off_window_formula_and_missing_merge_metadata_never_copy() {
    let mut complete = SpreadsheetApi::production();
    let sheet = complete.document().active_sheet();
    let a1 = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
    let z100000 = CellAddress::new(sheet, RowIndex(99_999), ColumnIndex(25));
    complete
        .set_cell_input(sheet, z100000, CellInput::Number(40.0))
        .unwrap();
    complete
        .set_cell_input(sheet, a1, CellInput::Formula("=Z100000+1".into()))
        .unwrap();
    let merge = CellRange::new(
        CellAddress::new(sheet, RowIndex(0), ColumnIndex(1)),
        CellAddress::new(sheet, RowIndex(0), ColumnIndex(3)),
    )
    .unwrap();
    complete.merge_cells(sheet, merge).unwrap();
    assert_eq!(complete.cell_view(sheet, a1).unwrap().display_text(), "41");
    let original = complete.export_xlsx().unwrap();
    let window = BoundedSheetWindow::initial();
    let partial = import_bounded_sheet_window(&original, window).unwrap();
    assert!(
        partial.sheets[0].merges.is_empty(),
        "bounded topology is unverified"
    );
    let visual = XlsxPartsCodec::export(&IroncalcXlsxParts, &partial).unwrap();
    let mut api = SpreadsheetApi::production();
    api.load_xlsx(&visual).unwrap();
    assert_eq!(
        api.cell_view(
            api.document().active_sheet(),
            CellAddress::new(api.document().active_sheet(), RowIndex(0), ColumnIndex(0))
        )
        .unwrap()
        .display_text(),
        "1"
    );
    let mut state = AppState::default();
    let session = DocumentSession::new(
        DocumentSource::from_path_with_retained_bytes(
            "off-window-dependency.xlsx".into(),
            Arc::from(original),
        ),
        OpenDocument::Sheets(api.document().snapshot()),
    )
    .unwrap();
    let id = state.documents.insert(session).unwrap();
    state.active_document = Some(id);
    state.bind_sheets_api(api).unwrap();
    state.sheets_bounded_window = Some(window);
    for cell in [(0, 0), (0, 2)] {
        select(&mut state, cell, cell);
        assert!(!state.sheets_copy_available());
        assert!(
            state
                .capture_sheets_clipboard(false)
                .unwrap_err()
                .contains("не подтверждены")
        );
    }
}
