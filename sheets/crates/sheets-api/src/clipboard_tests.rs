use crate::*;
use sheets_core::{
    AutoFilter, CellAddress, CellFormat, CellInput, CellRange, CellSnapshot, ChangeSet, Color,
    ColumnIndex, CoreError, EngineRequest, EngineResponse, FilterCriterion, Font, NumberFormat,
    RowIndex, SheetId, SpreadsheetEngine, Workbook, WorkbookSnapshot,
};
use sheets_ironcalc_adapter::IroncalcAdapter;
use std::{cell::Cell as StdCell, rc::Rc};

#[derive(Clone, Debug, Default)]
struct ClipboardEngineProbe(Rc<StdCell<usize>>);

impl ClipboardEngineProbe {
    fn calls(&self) -> usize {
        self.0.get()
    }
}

#[derive(Clone, Debug)]
struct ClipboardTestEngine(ClipboardEngineProbe);

impl SpreadsheetEngine for ClipboardTestEngine {
    type Error = CoreError;

    fn evaluate(&mut self, request: EngineRequest) -> Result<EngineResponse, Self::Error> {
        self.0.0.set(self.0.0.get().saturating_add(1));
        Ok(EngineResponse::new(request.base_revision(), Vec::new()))
    }
}

fn counting_api() -> (SpreadsheetApi<ClipboardTestEngine>, ClipboardEngineProbe) {
    let probe = ClipboardEngineProbe::default();
    (
        SpreadsheetApi::new(ClipboardTestEngine(probe.clone())),
        probe,
    )
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

fn assert_clipboard_error(error: &ApiError) {
    assert!(
        matches!(
            error.kind(),
            ApiErrorKind::Unsupported(ApiUnsupportedError::Clipboard(_))
        ),
        "expected a clipboard error, got {error:?}"
    );
}

fn assert_unchanged<E: SpreadsheetEngine>(
    api: &SpreadsheetApi<E>,
    before: &WorkbookSnapshot,
    history_before: HistoryStats,
) {
    assert_eq!(api.document().snapshot(), *before);
    assert_eq!(api.history_stats(), history_before);
}

fn evaluate_restored_snapshot<E: SpreadsheetEngine>(api: &mut SpreadsheetApi<E>) {
    let mut plan = api.document.workbook.prepare(ChangeSet::new()).unwrap();
    let response = api.engine.evaluate(plan.engine_request()).unwrap();
    api.document.workbook.commit(&mut plan, response).unwrap();
}

fn assert_cell_state<E: SpreadsheetEngine>(
    api: &SpreadsheetApi<E>,
    sheet: SheetId,
    row: u32,
    column: u32,
    input: Option<&CellInput>,
    format: Option<&CellFormat>,
) {
    let address = address(sheet, row, column);
    assert_eq!(api.document().cell_input(sheet, address), input);
    assert_eq!(api.document().cell_format(sheet, address), format);
}

fn assert_loaded_sparse_source_state(
    api: &SpreadsheetApi<IroncalcAdapter>,
    sheet: SheetId,
    literal_format: &CellFormat,
    formatted_blank: &CellFormat,
) {
    assert_cell_state(
        api,
        sheet,
        0,
        0,
        Some(&CellInput::Text("00123".into())),
        Some(literal_format),
    );
    assert_cell_state(api, sheet, 0, 1, Some(&CellInput::Number(7.0)), None);
    assert_cell_state(
        api,
        sheet,
        0,
        2,
        Some(&CellInput::formula("=B1*2").unwrap()),
        None,
    );
    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 0, 2)),
        Some(&CellValue::Number(14.0))
    );
    assert_cell_state(api, sheet, 0, 3, None, Some(formatted_blank));
    assert_cell_state(api, sheet, 0, 4, Some(&CellInput::Empty), None);
    assert_cell_state(api, sheet, 0, 5, None, None);
}

fn assert_pasted_sparse_state(
    api: &SpreadsheetApi<IroncalcAdapter>,
    sheet: SheetId,
    literal_format: &CellFormat,
    formatted_blank: &CellFormat,
) {
    assert_cell_state(
        api,
        sheet,
        2,
        0,
        Some(&CellInput::Text("00123".into())),
        Some(literal_format),
    );
    assert_cell_state(api, sheet, 2, 1, Some(&CellInput::Number(7.0)), None);
    assert_cell_state(
        api,
        sheet,
        2,
        2,
        Some(&CellInput::formula("=B3*2").unwrap()),
        None,
    );
    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 2, 2)),
        Some(&CellValue::Number(14.0))
    );
    assert_cell_state(api, sheet, 2, 3, None, Some(formatted_blank));
    assert_cell_state(api, sheet, 2, 4, Some(&CellInput::Empty), None);
    assert_cell_state(api, sheet, 2, 5, None, None);
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

fn loaded_clipboard_fixture() -> (
    SpreadsheetApi<IroncalcAdapter>,
    SheetId,
    CellFormat,
    CellFormat,
) {
    let mut source = SpreadsheetApi::production();
    let source_sheet = source.document().active_sheet();
    put_text(&mut source, source_sheet, 0, 0, "00123");
    put(&mut source, source_sheet, 0, 1, CellInput::Number(7.0));
    put_formula(&mut source, source_sheet, 0, 2, "=B1*2");

    let literal_format = CellFormat {
        font: Font {
            bold: true,
            ..Font::default()
        },
        background: Some(Color::new(10, 20, 30, 255)),
        ..CellFormat::default()
    };
    let formatted_blank = CellFormat {
        background: Some(Color::new(40, 50, 60, 255)),
        ..CellFormat::default()
    };
    source
        .set_cell_format(
            source_sheet,
            address(source_sheet, 0, 0),
            literal_format,
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
    source
        .set_cell_format(
            source_sheet,
            address(source_sheet, 0, 3),
            formatted_blank.clone(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
    let package = source.export_xlsx().unwrap();
    let mut loaded = SpreadsheetApi::production();
    loaded.load_xlsx(&package).unwrap();
    let sheet = loaded.document().active_sheet();
    let loaded_literal_format = loaded
        .document()
        .cell_format(sheet, address(sheet, 0, 0))
        .unwrap()
        .clone();
    let loaded_formatted_blank = loaded
        .document()
        .cell_format(sheet, address(sheet, 0, 3))
        .unwrap()
        .clone();

    assert_eq!(
        loaded.document().cell_input(sheet, address(sheet, 0, 0)),
        Some(&CellInput::Text("00123".into()))
    );
    assert_eq!(
        loaded
            .document()
            .cell_input(sheet, address(sheet, 0, 2))
            .and_then(CellInput::as_formula),
        Some("B1*2")
    );
    // Inject an unformatted explicit empty through the raw snapshot seam, then
    // evaluate the restored formula state with the production engine.
    let mut snapshot = loaded.document().snapshot();
    let sheet_snapshot = snapshot
        .sheets
        .iter_mut()
        .find(|candidate| candidate.id == sheet)
        .unwrap();
    sheet_snapshot
        .cells
        .retain(|cell| cell.address != address(sheet, 0, 3));
    sheet_snapshot.cells.push(CellSnapshot {
        address: address(sheet, 0, 3),
        input: CellInput::Empty,
        format: Some(loaded_formatted_blank.clone()),
    });
    sheet_snapshot.cells.push(CellSnapshot {
        address: address(sheet, 0, 4),
        input: CellInput::Empty,
        format: None,
    });
    loaded.document.workbook = Workbook::from_snapshot(snapshot).unwrap();
    evaluate_restored_snapshot(&mut loaded);

    // Restore D1 as a format-only cell (input=None) after the raw snapshot has
    // represented it as Some(Empty). E1 remains the unformatted Some(Empty).
    let formatted_blank_address = address(sheet, 0, 3);
    let mut changes = ChangeSet::new();
    crate::structural::replace_cell(
        &mut changes,
        formatted_blank_address,
        &crate::structural::CellFields {
            input: None,
            format: Some(loaded_formatted_blank.clone()),
        },
    );
    let mut plan = loaded.document.workbook.prepare(changes).unwrap();
    let response = loaded.engine.evaluate(plan.engine_request()).unwrap();
    loaded
        .document
        .workbook
        .commit(&mut plan, response)
        .unwrap();

    assert_cell_state(
        &loaded,
        sheet,
        0,
        2,
        Some(&CellInput::formula("=B1*2").unwrap()),
        None,
    );
    assert_eq!(
        loaded.document().cell_value(sheet, address(sheet, 0, 2)),
        Some(&CellValue::Number(14.0))
    );
    assert_cell_state(&loaded, sheet, 0, 3, None, Some(&loaded_formatted_blank));
    assert_cell_state(&loaded, sheet, 0, 4, Some(&CellInput::Empty), None);
    assert_cell_state(&loaded, sheet, 0, 5, None, None);

    (loaded, sheet, loaded_literal_format, loaded_formatted_blank)
}

#[test]
fn copy_is_observational_and_payload_reports_source() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "source");
    put_text(&mut api, sheet, 1, 0, "redo branch");
    api.undo().unwrap();
    assert!(api.can_redo());

    let selected = range(sheet, 0, 0, 0, 0);
    let before = api.document().snapshot();
    let history_before = api.history_stats();
    let payload = api.copy_range(selected.clone()).unwrap();

    assert_eq!(api.document().snapshot(), before);
    assert_eq!(api.history_stats(), history_before);
    assert!(api.can_redo());
    assert_eq!(payload.source_token(), api.document().token());
    assert_eq!(payload.source_revision(), before.revision);
    assert_eq!(payload.source_range(), &selected);
    assert_eq!(payload.rows(), 1);
    assert_eq!(payload.columns(), 1);
    assert!(payload.tsv().contains("source"));
}

#[test]
fn overlapping_paste_uses_the_copied_snapshot() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "A");
    put_text(&mut api, sheet, 0, 1, "B");
    let payload = api.copy_range(range(sheet, 0, 0, 0, 1)).unwrap();

    api.paste_range(&payload, range(sheet, 0, 1, 0, 2)).unwrap();

    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 0)),
        Some(&CellInput::Text("A".into()))
    );
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 1)),
        Some(&CellInput::Text("A".into()))
    );
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 2)),
        Some(&CellInput::Text("B".into()))
    );
}

#[test]
fn copy_formula_to_sheet_created_after_capture_rebases_local_reference() {
    let mut api = SpreadsheetApi::production();
    let source = api.document().active_sheet();
    put(&mut api, source, 0, 0, CellInput::Number(7.0));
    put_formula(&mut api, source, 0, 1, "=A1*2");
    let payload = api.copy_range(range(source, 0, 0, 0, 1)).unwrap();

    api.add_sheet(Some("Destination".into())).unwrap();
    let destination = api.document().active_sheet();
    api.paste_range(&payload, range(destination, 3, 3, 3, 4))
        .unwrap();

    assert_eq!(
        api.document()
            .cell_input(destination, address(destination, 3, 3)),
        Some(&CellInput::Number(7.0))
    );
    assert_eq!(
        api.document()
            .cell_input(destination, address(destination, 3, 4))
            .and_then(CellInput::as_formula),
        Some("D4*2")
    );
    assert_eq!(
        api.document()
            .cell_value(destination, address(destination, 3, 4)),
        Some(&CellValue::Number(14.0))
    );
    assert_eq!(
        api.document()
            .cell_input(source, address(source, 0, 1))
            .and_then(CellInput::as_formula),
        Some("A1*2")
    );
}

#[test]
fn copy_rejects_explicit_reference_to_sheet_renamed_after_capture_atomically() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(7.0));
    put_formula(&mut api, sheet, 0, 1, "=Sheet1!A1*2");
    let payload = api.copy_range(range(sheet, 0, 1, 0, 1)).unwrap();

    api.rename_sheet(sheet, "Renamed").unwrap();
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    let error = api
        .paste_range(&payload, range(sheet, 3, 3, 3, 3))
        .unwrap_err();

    assert_clipboard_error(&error);
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn loaded_rich_copy_round_trips_exact_sparse_states_and_styles() {
    let (mut api, sheet, literal_format, formatted_blank) = loaded_clipboard_fixture();
    assert_loaded_sparse_source_state(&api, sheet, &literal_format, &formatted_blank);

    let old_destination_format = CellFormat {
        font: Font {
            italic: true,
            ..Font::default()
        },
        background: Some(Color::new(80, 70, 60, 255)),
        ..CellFormat::default()
    };
    for (column, text) in [
        (0, "old literal destination"),
        (3, "old formatted blank destination"),
        (4, "old explicit empty destination"),
        (5, "old absent destination"),
    ] {
        put_text(&mut api, sheet, 2, column, text);
        api.set_cell_format(
            sheet,
            address(sheet, 2, column),
            old_destination_format.clone(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
    }
    let before_sheet = api
        .document()
        .snapshot()
        .sheets
        .into_iter()
        .find(|candidate| candidate.id == sheet)
        .unwrap();
    let history_before = api.history_stats();
    let payload = api.copy_range(range(sheet, 0, 0, 0, 5)).unwrap();

    assert!(payload.tsv().contains("00123"));
    assert!(payload.tsv().contains("14"));
    assert!(!payload.tsv().contains("B1*2"));
    api.paste_range(&payload, range(sheet, 2, 0, 2, 5)).unwrap();

    assert_eq!(
        api.history_stats().undo_entries,
        history_before.undo_entries + 1
    );
    assert_loaded_sparse_source_state(&api, sheet, &literal_format, &formatted_blank);
    assert_pasted_sparse_state(&api, sheet, &literal_format, &formatted_blank);

    api.undo_sheet(sheet).unwrap();
    let restored_sheet = api
        .document()
        .snapshot()
        .sheets
        .into_iter()
        .find(|candidate| candidate.id == sheet)
        .unwrap();
    assert_eq!(restored_sheet, before_sheet);
    assert_loaded_sparse_source_state(&api, sheet, &literal_format, &formatted_blank);
    for (column, text) in [
        (0, "old literal destination"),
        (3, "old formatted blank destination"),
        (4, "old explicit empty destination"),
        (5, "old absent destination"),
    ] {
        assert_cell_state(
            &api,
            sheet,
            2,
            column,
            Some(&CellInput::Text(text.into())),
            Some(&old_destination_format),
        );
    }
    assert_cell_state(&api, sheet, 2, 1, None, None);
    assert_cell_state(&api, sheet, 2, 2, None, None);

    api.redo_sheet(sheet).unwrap();
    assert_loaded_sparse_source_state(&api, sheet, &literal_format, &formatted_blank);
    assert_pasted_sparse_state(&api, sheet, &literal_format, &formatted_blank);
}

#[test]
fn rich_paste_copies_formatted_some_empty_over_same_style_none_once() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    let source = address(sheet, 0, 0);
    let destination = address(sheet, 0, 2);
    let format = CellFormat {
        background: Some(Color::new(18, 36, 54, 255)),
        number_format: NumberFormat::Decimal(2),
        ..CellFormat::default()
    };
    api.set_cell_format(
        sheet,
        source,
        format.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    put(&mut api, sheet, 0, 0, CellInput::Number(1.0));
    api.clear_cell(sheet, source).unwrap();
    api.set_cell_format(
        sheet,
        destination,
        format.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    assert_cell_state(&api, sheet, 0, 0, Some(&CellInput::Empty), Some(&format));
    assert_cell_state(&api, sheet, 0, 2, None, Some(&format));
    let history_before = api.history_stats();
    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();

    api.paste_range(&payload, range(sheet, 0, 2, 0, 2)).unwrap();

    assert_cell_state(&api, sheet, 0, 0, Some(&CellInput::Empty), Some(&format));
    assert_cell_state(&api, sheet, 0, 2, Some(&CellInput::Empty), Some(&format));
    assert_eq!(
        api.history_stats().undo_entries,
        history_before.undo_entries + 1
    );

    api.undo_sheet(sheet).unwrap();
    assert_cell_state(&api, sheet, 0, 0, Some(&CellInput::Empty), Some(&format));
    assert_cell_state(&api, sheet, 0, 2, None, Some(&format));
    api.redo_sheet(sheet).unwrap();
    assert_cell_state(&api, sheet, 0, 0, Some(&CellInput::Empty), Some(&format));
    assert_cell_state(&api, sheet, 0, 2, Some(&CellInput::Empty), Some(&format));
}

#[test]
fn rich_paste_copies_formatted_none_over_same_style_some_empty_once() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    let source = address(sheet, 0, 0);
    let destination = address(sheet, 0, 2);
    let format = CellFormat {
        background: Some(Color::new(18, 36, 54, 255)),
        number_format: NumberFormat::Decimal(2),
        ..CellFormat::default()
    };
    api.set_cell_format(
        sheet,
        source,
        format.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    api.set_cell_format(
        sheet,
        destination,
        format.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    put(&mut api, sheet, 0, 2, CellInput::Number(1.0));
    api.clear_cell(sheet, destination).unwrap();
    assert_cell_state(&api, sheet, 0, 0, None, Some(&format));
    assert_cell_state(&api, sheet, 0, 2, Some(&CellInput::Empty), Some(&format));
    let history_before = api.history_stats();
    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();

    api.paste_range(&payload, range(sheet, 0, 2, 0, 2)).unwrap();

    assert_cell_state(&api, sheet, 0, 0, None, Some(&format));
    assert_cell_state(&api, sheet, 0, 2, None, Some(&format));
    assert_eq!(
        api.history_stats().undo_entries,
        history_before.undo_entries + 1
    );

    api.undo_sheet(sheet).unwrap();
    assert_cell_state(&api, sheet, 0, 0, None, Some(&format));
    assert_cell_state(&api, sheet, 0, 2, Some(&CellInput::Empty), Some(&format));
    api.redo_sheet(sheet).unwrap();
    assert_cell_state(&api, sheet, 0, 0, None, Some(&format));
    assert_cell_state(&api, sheet, 0, 2, None, Some(&format));
}

#[test]
fn rich_paste_clears_destination_for_an_absent_source_cell() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "copied");
    put_text(&mut api, sheet, 0, 3, "old value");
    let old_format = CellFormat {
        background: Some(Color::new(90, 80, 70, 255)),
        ..CellFormat::default()
    };
    api.set_cell_format(
        sheet,
        address(sheet, 0, 3),
        old_format,
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    let payload = api.copy_range(range(sheet, 0, 0, 0, 1)).unwrap();

    api.paste_range(&payload, range(sheet, 0, 3, 0, 4)).unwrap();

    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 3)),
        Some(&CellInput::Text("copied".into()))
    );
    assert_eq!(api.document().cell_input(sheet, address(sheet, 0, 4)), None);
    assert_eq!(
        api.document().cell_format(sheet, address(sheet, 0, 4)),
        None
    );
}

#[test]
fn rich_paste_requires_matching_shape_and_rejects_mismatched_multicell_shape() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    for (row, column, text) in [(0, 0, "a"), (0, 1, "b"), (1, 0, "c"), (1, 1, "d")] {
        put_text(&mut api, sheet, row, column, text);
    }
    let source = range(sheet, 0, 0, 1, 1);
    let payload = api.copy_range(source.clone()).unwrap();
    api.paste_range(&payload, range(sheet, 3, 3, 4, 4)).unwrap();
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 4, 4)),
        Some(&CellInput::Text("d".into()))
    );

    let fresh_payload = api.copy_range(source).unwrap();
    let before = api.document().snapshot();
    let history_before = api.history_stats();
    let error = api
        .paste_range(&fresh_payload, range(sheet, 7, 7, 7, 8))
        .unwrap_err();
    assert_clipboard_error(&error);
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn external_tsv_preserves_literal_intent_and_decodes_quoted_fields() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    let text = "00123\t+123\t10\r\n1-2\t\"tab\tfield\"\t\"line\nbreak\"\r\n  spaced  \ttext ";

    api.paste_tsv(text, range(sheet, 0, 0, 2, 2)).unwrap();

    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 0)),
        Some(&CellInput::Text("00123".into()))
    );
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 1)),
        Some(&CellInput::Text("+123".into()))
    );
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 2)),
        Some(&CellInput::Number(10.0))
    );
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 1, 0)),
        Some(&CellInput::Text("1-2".into()))
    );
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 1, 1)),
        Some(&CellInput::Text("tab\tfield".into()))
    );
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 1, 2)),
        Some(&CellInput::Text("line\nbreak".into()))
    );
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 2, 0)),
        Some(&CellInput::Text("  spaced  ".into()))
    );
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 2, 1)),
        Some(&CellInput::Text("text ".into()))
    );
}

#[test]
fn external_tsv_formula_uses_the_canonical_formula_input_path() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();

    api.paste_tsv("=1+2", range(sheet, 0, 0, 0, 0)).unwrap();

    assert_eq!(
        api.document()
            .cell_input(sheet, address(sheet, 0, 0))
            .and_then(CellInput::as_formula),
        Some("1+2")
    );
}

#[test]
fn copy_exports_displayed_formula_value_and_preserves_number_format() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(7.0));
    put_formula(&mut api, sheet, 0, 1, "=A1*2");
    let formula_format = CellFormat {
        number_format: NumberFormat::Decimal(2),
        ..CellFormat::default()
    };
    api.set_cell_format(
        sheet,
        address(sheet, 0, 1),
        formula_format.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();

    let payload = api.copy_range(range(sheet, 0, 0, 0, 1)).unwrap();
    assert!(payload.tsv().contains("14.00"));
    assert!(!payload.tsv().contains("A1*2"));
    api.paste_range(&payload, range(sheet, 3, 3, 3, 4)).unwrap();

    assert_eq!(
        api.document()
            .cell_input(sheet, address(sheet, 3, 4))
            .and_then(CellInput::as_formula),
        Some("D4*2")
    );
    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 3, 4)),
        Some(&CellValue::Number(14.0))
    );
    assert_eq!(
        api.document().cell_format(sheet, address(sheet, 3, 4)),
        Some(&formula_format)
    );
    assert_eq!(
        api.cell_view(sheet, address(sheet, 3, 4))
            .unwrap()
            .display_text(),
        "14.00"
    );
}

#[test]
fn formula_with_empty_text_result_exports_a_blank_field() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_formula(&mut api, sheet, 0, 0, "=IF(FALSE,1,\"\")");
    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 0, 0)),
        Some(&CellValue::Text(String::new()))
    );

    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();

    assert_eq!(payload.tsv(), "\"\"\r\n");
    assert!(!payload.tsv().contains("IF(FALSE"));
}

#[test]
fn external_tsv_shape_rejection_is_atomic() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 4, 4, "keep");
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    let error = api
        .paste_tsv("a\tb\nc\td", range(sheet, 4, 4, 4, 5))
        .unwrap_err();

    assert_clipboard_error(&error);
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn copy_and_paste_preserve_complete_merged_ranges() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "merged");
    let source = range(sheet, 0, 0, 0, 1);
    let destination = range(sheet, 0, 3, 0, 4);
    api.safe_merge_cells(sheet, source.clone()).unwrap();
    let payload = api.copy_range(source.clone()).unwrap();

    api.paste_range(&payload, destination.clone()).unwrap();

    let merges = api.document().snapshot().sheets[0].merges.clone();
    assert!(merges.contains(&source));
    assert!(merges.contains(&destination));
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 3)),
        Some(&CellInput::Text("merged".into()))
    );
}

#[test]
fn copy_rejects_a_partial_merged_range_atomically() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    api.safe_merge_cells(sheet, range(sheet, 0, 0, 1, 1))
        .unwrap();
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    let error = api.copy_range(range(sheet, 0, 0, 1, 0)).unwrap_err();

    assert_clipboard_error(&error);
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn paste_rejects_a_collision_with_an_existing_merge_atomically() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    let source = range(sheet, 0, 0, 0, 1);
    api.safe_merge_cells(sheet, source.clone()).unwrap();
    api.safe_merge_cells(sheet, range(sheet, 0, 3, 1, 4))
        .unwrap();
    let payload = api.copy_range(source).unwrap();
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    let error = api
        .paste_range(&payload, range(sheet, 0, 3, 0, 4))
        .unwrap_err();

    assert_clipboard_error(&error);
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn copy_includes_filtered_and_manually_hidden_rows() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    for (row, id, status) in [
        (0, "id", "status"),
        (1, "manual", "keep"),
        (2, "filtered", "drop"),
    ] {
        put_text(&mut api, sheet, row, 0, id);
        put_text(&mut api, sheet, row, 1, status);
    }
    let table = range(sheet, 0, 0, 2, 1);
    api.set_auto_filter(
        sheet,
        filter(
            table.clone(),
            true,
            [(1, FilterCriterion::Equals("keep".into()))],
        ),
    )
    .unwrap();
    api.set_manual_hidden_rows(sheet, vec![RowIndex(1)])
        .unwrap();
    assert_eq!(api.filter_hidden_rows(sheet), vec![RowIndex(2)]);
    assert_eq!(api.manual_hidden_rows(sheet), vec![RowIndex(1)]);

    let payload = api.copy_range(table).unwrap();
    api.paste_range(&payload, range(sheet, 0, 3, 2, 4)).unwrap();

    for (row, id, status) in [
        (0, "id", "status"),
        (1, "manual", "keep"),
        (2, "filtered", "drop"),
    ] {
        assert_eq!(
            api.document().cell_input(sheet, address(sheet, row, 3)),
            Some(&CellInput::Text(id.into()))
        );
        assert_eq!(
            api.document().cell_input(sheet, address(sheet, row, 4)),
            Some(&CellInput::Text(status.into()))
        );
    }
    assert_eq!(api.filter_hidden_rows(sheet), vec![RowIndex(2)]);
    assert_eq!(api.manual_hidden_rows(sheet), vec![RowIndex(1)]);
}

#[test]
fn paste_recomputes_filter_visibility_and_keeps_manual_hidden_rows() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "status");
    put_text(&mut api, sheet, 1, 0, "hide");
    put_text(&mut api, sheet, 2, 0, "keep");
    api.set_auto_filter(
        sheet,
        filter(
            range(sheet, 0, 0, 2, 0),
            true,
            [(0, FilterCriterion::Equals("keep".into()))],
        ),
    )
    .unwrap();
    api.set_manual_hidden_rows(sheet, vec![RowIndex(2)])
        .unwrap();
    assert_eq!(api.filter_hidden_rows(sheet), vec![RowIndex(1)]);

    api.paste_tsv("keep", range(sheet, 1, 0, 1, 0)).unwrap();
    assert!(api.filter_hidden_rows(sheet).is_empty());
    assert_eq!(api.manual_hidden_rows(sheet), vec![RowIndex(2)]);

    api.paste_tsv("hide", range(sheet, 1, 0, 1, 0)).unwrap();
    assert_eq!(api.filter_hidden_rows(sheet), vec![RowIndex(1)]);
    assert_eq!(api.manual_hidden_rows(sheet), vec![RowIndex(2)]);
}

#[test]
fn cut_payload_stays_valid_after_active_sheet_navigation() {
    let mut api = SpreadsheetApi::production();
    let source = api.document().active_sheet();
    put_text(&mut api, source, 0, 0, "move me");
    api.add_sheet(Some("Other".into())).unwrap();
    let other = api
        .document()
        .snapshot()
        .sheets
        .iter()
        .find(|sheet| sheet.name == "Other")
        .unwrap()
        .id;
    api.set_active_sheet(source).unwrap();
    let payload = api.copy_range(range(source, 0, 0, 0, 0)).unwrap();
    assert!(api.cut_payload_valid(&payload));

    api.set_active_sheet(other).unwrap();

    assert!(api.cut_payload_valid(&payload));
}

#[test]
fn cut_payload_invalidates_after_source_mutation_and_rejects_atomically() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "before");
    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();
    put_text(&mut api, sheet, 0, 0, "changed");
    assert!(!api.cut_payload_valid(&payload));
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    let error = api
        .cut_paste(&payload, range(sheet, 0, 1, 0, 1))
        .unwrap_err();

    assert_clipboard_error(&error);
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn successful_load_invalidates_pending_cut_even_when_revision_repeats() {
    let mut api = SpreadsheetApi::production();
    let source_sheet = api.document().active_sheet();
    put_text(&mut api, source_sheet, 1, 0, "source before load");
    let payload = api.copy_range(range(source_sheet, 0, 0, 0, 0)).unwrap();
    let token = payload.source_token();
    let revision = payload.source_revision();

    let mut replacement = SpreadsheetApi::production();
    let replacement_sheet = replacement.document().active_sheet();
    put_text(
        &mut replacement,
        replacement_sheet,
        0,
        0,
        "loaded replacement",
    );
    let package = replacement.export_xlsx().unwrap();
    api.load_xlsx(&package).unwrap();

    let loaded_sheet = api.document().active_sheet();
    assert_eq!(api.document().token(), token);
    assert_eq!(api.document().revision(), revision);
    assert_eq!(
        api.document()
            .cell_input(loaded_sheet, address(loaded_sheet, 0, 0)),
        Some(&CellInput::Text("loaded replacement".into()))
    );
    assert!(!api.cut_payload_valid(&payload));
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    let error = api
        .cut_paste(&payload, range(loaded_sheet, 0, 1, 0, 1))
        .unwrap_err();

    assert_clipboard_error(&error);
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn undo_and_redo_invalidate_pending_cut_even_when_source_is_restored() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "source");
    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();
    assert!(api.cut_payload_valid(&payload));

    api.undo().unwrap();
    assert_eq!(api.document().cell_input(sheet, address(sheet, 0, 0)), None);
    assert!(!api.cut_payload_valid(&payload));

    api.redo().unwrap();
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 0)),
        Some(&CellInput::Text("source".into()))
    );
    assert!(!api.cut_payload_valid(&payload));
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    let error = api
        .cut_paste(&payload, range(sheet, 0, 1, 0, 1))
        .unwrap_err();

    assert_clipboard_error(&error);
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn same_sheet_cut_is_one_undoable_transaction() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "00123");
    put_text(&mut api, sheet, 0, 1, "old destination");
    let source_format = CellFormat {
        font: Font {
            italic: true,
            ..Font::default()
        },
        ..CellFormat::default()
    };
    api.set_cell_format(
        sheet,
        address(sheet, 0, 0),
        source_format.clone(),
        FormatPatchBounds {
            max_cells: 1,
            max_sheets: 1,
        },
    )
    .unwrap();
    let history_before = api.history_stats();
    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();

    api.cut_paste(&payload, range(sheet, 0, 1, 0, 1)).unwrap();

    assert_eq!(
        api.history_stats().undo_entries,
        history_before.undo_entries + 1
    );
    assert_eq!(api.document().cell_input(sheet, address(sheet, 0, 0)), None);
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 1)),
        Some(&CellInput::Text("00123".into()))
    );
    assert_eq!(
        api.document().cell_format(sheet, address(sheet, 0, 0)),
        None
    );
    assert_eq!(
        api.document().cell_format(sheet, address(sheet, 0, 1)),
        Some(&source_format)
    );

    api.undo_sheet(sheet).unwrap();
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 0)),
        Some(&CellInput::Text("00123".into()))
    );
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 1)),
        Some(&CellInput::Text("old destination".into()))
    );
    assert_eq!(
        api.document().cell_format(sheet, address(sheet, 0, 0)),
        Some(&source_format)
    );
    api.redo_sheet(sheet).unwrap();
    assert_eq!(api.document().cell_input(sheet, address(sheet, 0, 0)), None);
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 1)),
        Some(&CellInput::Text("00123".into()))
    );
}

#[test]
fn cross_sheet_cut_is_one_destination_owned_undo_entry() {
    let mut api = SpreadsheetApi::production();
    let source = api.document().active_sheet();
    put_text(&mut api, source, 0, 0, "literal move");
    api.add_sheet(Some("Destination".into())).unwrap();
    let destination = api.document().active_sheet();
    let payload = api.copy_range(range(source, 0, 0, 0, 0)).unwrap();
    let history_before = api.history_stats();

    api.cut_paste(&payload, range(destination, 2, 1, 2, 1))
        .unwrap();

    assert_eq!(
        api.history_stats().undo_entries,
        history_before.undo_entries + 1
    );
    assert!(api.can_undo_sheet(destination));
    assert_eq!(
        api.document().cell_input(source, address(source, 0, 0)),
        None
    );
    assert_eq!(
        api.document()
            .cell_input(destination, address(destination, 2, 1)),
        Some(&CellInput::Text("literal move".into()))
    );

    api.undo_sheet(destination).unwrap();
    assert_eq!(
        api.document().cell_input(source, address(source, 0, 0)),
        Some(&CellInput::Text("literal move".into()))
    );
    assert_eq!(
        api.document()
            .cell_input(destination, address(destination, 2, 1)),
        None
    );
    api.redo_sheet(destination).unwrap();
    assert_eq!(
        api.document().cell_input(source, address(source, 0, 0)),
        None
    );
    assert_eq!(
        api.document()
            .cell_input(destination, address(destination, 2, 1)),
        Some(&CellInput::Text("literal move".into()))
    );
}

#[test]
fn cut_rejects_overlapping_source_and_destination_atomically() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "left");
    put_text(&mut api, sheet, 0, 1, "right");
    let payload = api.copy_range(range(sheet, 0, 0, 0, 1)).unwrap();
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    let error = api
        .cut_paste(&payload, range(sheet, 0, 1, 0, 2))
        .unwrap_err();

    assert_clipboard_error(&error);
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn cut_rejection_when_history_budget_is_too_small_is_atomic() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "source");
    put_text(&mut api, sheet, 0, 1, "destination");
    api.set_history_limits(HistoryLimits {
        max_entries: 10,
        max_retained_cells: 1,
        max_retained_bytes: 1024 * 1024,
    });
    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    let error = api
        .cut_paste(&payload, range(sheet, 0, 1, 0, 1))
        .unwrap_err();

    assert!(matches!(
        error.kind(),
        ApiErrorKind::Transaction(ApiTransactionError::HistoryBudgetExceeded { .. })
    ));
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn same_sheet_cut_rewrites_local_observer_and_round_trips_one_undo_entry() {
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    put(&mut api, sheet, 0, 0, CellInput::Number(7.0));
    put_formula(&mut api, sheet, 0, 2, "=A1*2");
    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();
    let history_before = api.history_stats();

    api.cut_paste(&payload, range(sheet, 3, 3, 3, 3)).unwrap();

    assert_eq!(
        api.history_stats().undo_entries,
        history_before.undo_entries + 1
    );
    assert_eq!(api.document().cell_input(sheet, address(sheet, 0, 0)), None);
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 3, 3)),
        Some(&CellInput::Number(7.0))
    );
    assert_eq!(
        api.document()
            .cell_input(sheet, address(sheet, 0, 2))
            .and_then(CellInput::as_formula),
        Some("D4*2")
    );
    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 0, 2)),
        Some(&CellValue::Number(14.0))
    );

    api.undo_sheet(sheet).unwrap();
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 0)),
        Some(&CellInput::Number(7.0))
    );
    assert_eq!(api.document().cell_input(sheet, address(sheet, 3, 3)), None);
    assert_eq!(
        api.document()
            .cell_input(sheet, address(sheet, 0, 2))
            .and_then(CellInput::as_formula),
        Some("A1*2")
    );
    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 0, 2)),
        Some(&CellValue::Number(14.0))
    );

    api.redo_sheet(sheet).unwrap();
    assert_eq!(api.document().cell_input(sheet, address(sheet, 0, 0)), None);
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 3, 3)),
        Some(&CellInput::Number(7.0))
    );
    assert_eq!(
        api.document()
            .cell_input(sheet, address(sheet, 0, 2))
            .and_then(CellInput::as_formula),
        Some("D4*2")
    );
    assert_eq!(
        api.document().cell_value(sheet, address(sheet, 0, 2)),
        Some(&CellValue::Number(14.0))
    );
}

#[test]
fn same_sheet_cut_rewrites_an_observer_on_another_sheet() {
    let mut api = SpreadsheetApi::production();
    let source = api.document().active_sheet();
    put(&mut api, source, 0, 0, CellInput::Number(7.0));
    api.add_sheet(Some("Observer".into())).unwrap();
    let observer = api.document().active_sheet();
    put_formula(&mut api, observer, 0, 2, "=Sheet1!A1*2");
    api.set_active_sheet(source).unwrap();
    let payload = api.copy_range(range(source, 0, 0, 0, 0)).unwrap();
    let history_before = api.history_stats();

    api.cut_paste(&payload, range(source, 3, 3, 3, 3)).unwrap();

    assert_eq!(
        api.history_stats().undo_entries,
        history_before.undo_entries + 1
    );
    assert_eq!(
        api.document()
            .cell_input(observer, address(observer, 0, 2))
            .and_then(CellInput::as_formula),
        Some("Sheet1!D4*2")
    );
    assert_eq!(
        api.document().cell_value(observer, address(observer, 0, 2)),
        Some(&CellValue::Number(14.0))
    );

    api.undo_sheet(source).unwrap();
    assert_eq!(
        api.document()
            .cell_input(observer, address(observer, 0, 2))
            .and_then(CellInput::as_formula),
        Some("Sheet1!A1*2")
    );
    assert_eq!(
        api.document().cell_value(observer, address(observer, 0, 2)),
        Some(&CellValue::Number(14.0))
    );

    api.redo_sheet(source).unwrap();
    assert_eq!(
        api.document()
            .cell_input(observer, address(observer, 0, 2))
            .and_then(CellInput::as_formula),
        Some("Sheet1!D4*2")
    );
    assert_eq!(
        api.document().cell_value(observer, address(observer, 0, 2)),
        Some(&CellValue::Number(14.0))
    );
}

#[test]
fn cross_sheet_cut_rejects_an_external_formula_dependency_atomically() {
    let mut api = SpreadsheetApi::production();
    let source = api.document().active_sheet();
    put(&mut api, source, 0, 0, CellInput::Number(7.0));
    api.add_sheet(Some("Observer".into())).unwrap();
    let observer = api.document().active_sheet();
    put_formula(&mut api, observer, 0, 2, "=Sheet1!A1*2");
    api.set_active_sheet(source).unwrap();
    let payload = api.copy_range(range(source, 0, 0, 0, 0)).unwrap();
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    let error = api
        .cut_paste(&payload, range(observer, 3, 3, 3, 3))
        .unwrap_err();

    assert_clipboard_error(&error);
    assert_unchanged(&api, &before, history_before);
}

#[test]
fn cross_document_copy_rebases_local_formula_and_preserves_format() {
    let mut source = SpreadsheetApi::production();
    let source_sheet = source.document().active_sheet();
    put(&mut source, source_sheet, 0, 0, CellInput::Number(5.0));
    put_formula(&mut source, source_sheet, 0, 1, "=A1*2");
    let formula_format = CellFormat {
        font: Font {
            bold: true,
            ..Font::default()
        },
        background: Some(Color::new(22, 44, 66, 255)),
        ..CellFormat::default()
    };
    source
        .set_cell_format(
            source_sheet,
            address(source_sheet, 0, 1),
            formula_format.clone(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
    let source_before = source.document().snapshot();
    let payload = source.copy_range(range(source_sheet, 0, 0, 0, 1)).unwrap();

    let mut target = SpreadsheetApi::production();
    let target_sheet = target.document().active_sheet();
    target
        .paste_range(&payload, range(target_sheet, 2, 3, 2, 4))
        .unwrap();

    assert_eq!(source.document().snapshot(), source_before);
    assert_ne!(payload.source_token(), target.document().token());
    assert_eq!(
        target
            .document()
            .cell_input(target_sheet, address(target_sheet, 2, 4))
            .and_then(CellInput::as_formula),
        Some("D3*2")
    );
    assert_eq!(
        target
            .document()
            .cell_format(target_sheet, address(target_sheet, 2, 4)),
        Some(&formula_format)
    );
}

#[test]
fn cross_document_copy_rejects_explicit_sheet_references_atomically() {
    let mut source = SpreadsheetApi::production();
    let source_sheet = source.document().active_sheet();
    put(&mut source, source_sheet, 0, 0, CellInput::Number(5.0));
    put_formula(&mut source, source_sheet, 0, 1, "=Sheet1!A1");
    let payload = source.copy_range(range(source_sheet, 0, 1, 0, 1)).unwrap();
    let source_before = source.document().snapshot();

    let mut target = SpreadsheetApi::production();
    let target_sheet = target.document().active_sheet();
    let target_before = target.document().snapshot();
    let history_before = target.history_stats();
    let error = target
        .paste_range(&payload, range(target_sheet, 2, 2, 2, 2))
        .unwrap_err();

    assert_clipboard_error(&error);
    assert_eq!(source.document().snapshot(), source_before);
    assert_unchanged(&target, &target_before, history_before);
}

#[test]
fn cross_document_cut_rejects_without_changing_either_workbook() {
    let mut source = SpreadsheetApi::production();
    let source_sheet = source.document().active_sheet();
    put_text(&mut source, source_sheet, 0, 0, "source text");
    let payload = source.copy_range(range(source_sheet, 0, 0, 0, 0)).unwrap();
    let source_before = source.document().snapshot();

    let mut target = SpreadsheetApi::production();
    let target_sheet = target.document().active_sheet();
    let target_before = target.document().snapshot();
    let history_before = target.history_stats();
    assert!(!target.cut_payload_valid(&payload));
    let error = target
        .cut_paste(&payload, range(target_sheet, 0, 0, 0, 0))
        .unwrap_err();

    assert_clipboard_error(&error);
    assert_eq!(source.document().snapshot(), source_before);
    assert_unchanged(&target, &target_before, history_before);
}

#[test]
fn copy_small_range_in_large_workbook_preserves_redo() {
    let (mut api, probe) = counting_api();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "selected");
    put_text(&mut api, sheet, 1, 0, "redo marker");
    api.undo().unwrap();
    assert!(api.can_redo());

    let large_bytes = 16 * 1024 * 1024 + 1;
    let mut snapshot = api.document().snapshot();
    let sheet_snapshot = snapshot
        .sheets
        .iter_mut()
        .find(|candidate| candidate.id == sheet)
        .unwrap();
    sheet_snapshot.cells.push(CellSnapshot {
        address: address(sheet, 0, 2),
        input: CellInput::Text("u".repeat(large_bytes)),
        format: None,
    });
    api.document.workbook = Workbook::from_snapshot(snapshot).unwrap();

    let revision_before = api.document().revision();
    let history_before = api.history_stats();
    let calls_before = probe.calls();
    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();

    assert_eq!(payload.tsv(), "selected\r\n");
    assert_eq!(api.document().revision(), revision_before);
    assert_eq!(api.history_stats(), history_before);
    assert_eq!(probe.calls(), calls_before);
    assert!(api.can_redo());
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 0)),
        Some(&CellInput::Text("selected".into()))
    );
    assert!(matches!(
        api.document().cell_input(sheet, address(sheet, 0, 2)),
        Some(CellInput::Text(text)) if text.len() == large_bytes
            && text.as_bytes().first() == Some(&b'u')
            && text.as_bytes().last() == Some(&b'u')
    ));
}

#[test]
fn cut_with_oversized_canonical_workbook_rejects_before_engine_evaluation() {
    let (mut api, probe) = counting_api();
    let sheet = api.document().active_sheet();
    let large_bytes = 16 * 1024 * 1024 + 1;
    let mut snapshot = api.document().snapshot();
    let sheet_snapshot = snapshot
        .sheets
        .iter_mut()
        .find(|candidate| candidate.id == sheet)
        .unwrap();
    sheet_snapshot.cells.push(CellSnapshot {
        address: address(sheet, 0, 0),
        input: CellInput::Text("small source".into()),
        format: None,
    });
    sheet_snapshot.cells.push(CellSnapshot {
        address: address(sheet, 0, 2),
        input: CellInput::Text("v".repeat(large_bytes)),
        format: None,
    });
    api.document.workbook = Workbook::from_snapshot(snapshot).unwrap();
    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();
    assert!(api.cut_payload_valid(&payload));

    let revision_before = api.document().revision();
    let history_before = api.history_stats();
    let calls_before = probe.calls();
    assert!(
        api.cut_paste(&payload, range(sheet, 3, 3, 3, 3)).is_err(),
        "a cut over the 16 MiB canonical payload bound must reject"
    );

    assert_eq!(api.document().revision(), revision_before);
    assert_eq!(api.history_stats(), history_before);
    assert_eq!(probe.calls(), calls_before);
    assert!(api.cut_payload_valid(&payload));
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 0)),
        Some(&CellInput::Text("small source".into()))
    );
    assert_eq!(api.document().cell_input(sheet, address(sheet, 3, 3)), None);
    assert!(matches!(
        api.document().cell_input(sheet, address(sheet, 0, 2)),
        Some(CellInput::Text(text)) if text.len() == large_bytes
            && text.as_bytes().first() == Some(&b'v')
            && text.as_bytes().last() == Some(&b'v')
    ));
}

#[test]
fn oversized_destination_paste_rejects_before_engine_and_preserves_redo() {
    let (mut api, probe) = counting_api();
    let sheet = api.document().active_sheet();
    put_text(&mut api, sheet, 0, 0, "small source");
    put_text(&mut api, sheet, 0, 2, "redo marker");
    api.undo().unwrap();

    let old_text = "w".repeat(2 * 1024 * 1024);
    let mut snapshot = api.document().snapshot();
    let sheet_snapshot = snapshot
        .sheets
        .iter_mut()
        .find(|candidate| candidate.id == sheet)
        .unwrap();
    sheet_snapshot.cells.push(CellSnapshot {
        address: address(sheet, 0, 1),
        input: CellInput::Text(old_text.clone()),
        format: None,
    });
    api.document.workbook = Workbook::from_snapshot(snapshot).unwrap();
    api.set_history_limits(HistoryLimits {
        max_entries: 10,
        max_retained_cells: 100,
        max_retained_bytes: 1024 * 1024,
    });
    let payload = api.copy_range(range(sheet, 0, 0, 0, 0)).unwrap();

    let revision_before = api.document().revision();
    let history_before = api.history_stats();
    let calls_before = probe.calls();
    let error = api
        .paste_range(&payload, range(sheet, 0, 1, 0, 1))
        .unwrap_err();

    assert!(matches!(
        error.kind(),
        ApiErrorKind::Transaction(ApiTransactionError::HistoryBudgetExceeded { .. })
    ));
    assert_eq!(api.document().revision(), revision_before);
    assert_eq!(api.history_stats(), history_before);
    assert_eq!(probe.calls(), calls_before);
    assert!(api.can_redo());
    assert_eq!(
        api.document().cell_input(sheet, address(sheet, 0, 0)),
        Some(&CellInput::Text("small source".into()))
    );
    assert!(matches!(
        api.document().cell_input(sheet, address(sheet, 0, 1)),
        Some(CellInput::Text(text)) if text == &old_text
    ));
    assert_eq!(api.document().cell_input(sheet, address(sheet, 0, 2)), None);
}

#[test]
fn copy_rejects_an_oversized_range_without_mutation() {
    let api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    let before = api.document().snapshot();
    let history_before = api.history_stats();

    assert!(api.copy_range(range(sheet, 0, 0, 99, 100)).is_err());

    assert_unchanged(&api, &before, history_before);
}
