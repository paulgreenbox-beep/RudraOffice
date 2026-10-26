use super::*;

fn cell(row: u32, column: u32) -> CellAddress {
    CellAddress::new(SheetId(0), RowIndex(row), ColumnIndex(column))
}

fn small_ui(rows: u32, columns: u32) -> SheetsUi {
    SheetsUi::new(UiConfig {
        dimensions: GridDimensions { rows, columns },
        outer_width: 700,
        outer_height: 500,
        default_row_height: 20,
        default_column_width: 40,
        ..UiConfig::default()
    })
    .unwrap()
}

fn key(ui: &mut SheetsUi, key: Key, shift: bool) {
    ui.handle_event(UiEvent::KeyDown {
        key,
        modifiers: Modifiers {
            shift,
            ..Modifiers::default()
        },
    })
    .unwrap();
}

#[test]
fn hidden_columns_are_omitted_from_cells_headers_hits_and_content_extent() {
    let mut ui = small_ui(6, 8);
    ui.apply_hidden_columns(&[ColumnIndex(1), ColumnIndex(2)])
        .unwrap();

    let frame = ui.frame().unwrap();
    assert!(
        !frame
            .column_headers
            .iter()
            .any(|header| header.index == 1 || header.index == 2)
    );
    assert!(
        !frame
            .cells
            .iter()
            .any(|rendered| rendered.address.column.0 == 1 || rendered.address.column.0 == 2)
    );
    assert!(frame.column_headers.iter().any(|header| header.index == 3));
    let address = cell(0, 3);
    let rect = ui.grid.cell_rect(address).unwrap();
    assert_eq!(rect.x, 40.0);
    let point = Point::new(
        ui.layout.grid_viewport.x + rect.x + 1.0,
        ui.layout.grid_viewport.y + 1.0,
    );
    assert_eq!(ui.hit_test_grid(point), Some(address));
    assert_eq!(
        frame.content_width,
        ui.grid
            .layout(ui.logical_grid_viewport(), ui.scroll)
            .content_width as f64
    );
}

#[test]
fn metadata_replacement_drops_stale_axis_overrides_then_applies_shifted_sizes() {
    let mut ui = small_ui(100, 100);
    ui.apply_sheet_metadata(
        &[RowDimension {
            row: RowIndex(1),
            height: 45.0,
        }],
        &[ColumnDimension {
            column: ColumnIndex(2),
            width: 30.0,
        }],
        &[],
    )
    .unwrap();
    assert_eq!(ui.grid.cell_rect(cell(2, 0)).unwrap().y, 65.0);
    assert_eq!(ui.grid.cell_rect(cell(0, 3)).unwrap().x, 110.0);

    ui.handle_event(UiEvent::Wheel {
        delta_x: 300,
        delta_y: 300,
    })
    .unwrap();
    let scroll = ui.scroll_offset();
    ui.apply_sheet_metadata(
        &[RowDimension {
            row: RowIndex(6),
            height: 27.0,
        }],
        &[ColumnDimension {
            column: ColumnIndex(7),
            width: 25.0,
        }],
        &[],
    )
    .unwrap();

    assert_eq!(ui.grid.cell_rect(cell(2, 0)).unwrap().y, 40.0);
    assert_eq!(ui.grid.cell_rect(cell(0, 3)).unwrap().x, 120.0);
    assert_eq!(ui.grid.cell_rect(cell(7, 0)).unwrap().y, 147.0);
    assert_eq!(ui.grid.cell_rect(cell(0, 8)).unwrap().x, 305.0);
    assert_eq!(ui.scroll_offset(), scroll);
    let layout = ui.grid.layout(ui.logical_grid_viewport(), ui.scroll);
    assert!(u128::from(ui.scroll.x) <= layout.content_width);
    assert!(u128::from(ui.scroll.y) <= layout.content_height);
}

#[test]
fn metadata_replacement_preserves_the_supplied_hidden_axis_projections() {
    let mut ui = small_ui(8, 8);
    ui.apply_hidden_rows(&[RowIndex(2)]).unwrap();
    ui.apply_hidden_columns(&[ColumnIndex(3)]).unwrap();

    ui.apply_sheet_metadata(
        &[RowDimension {
            row: RowIndex(4),
            height: 30.0,
        }],
        &[ColumnDimension {
            column: ColumnIndex(5),
            width: 50.0,
        }],
        &[],
    )
    .unwrap();

    assert!(ui.is_row_hidden(RowIndex(2)));
    assert!(ui.is_column_hidden(ColumnIndex(3)));
    assert_eq!(ui.grid.cell_rect(cell(5, 0)).unwrap().y, 90.0);
    assert_eq!(ui.grid.cell_rect(cell(0, 6)).unwrap().x, 210.0);
}

#[test]
fn horizontal_navigation_skips_hidden_columns_but_keeps_logical_selection() {
    let mut ui = small_ui(8, 8);
    ui.select_cell(cell(2, 0)).unwrap();
    ui.apply_hidden_columns(&[ColumnIndex(1), ColumnIndex(2)])
        .unwrap();
    key(&mut ui, Key::ArrowRight, false);
    assert_eq!(ui.selection.focus, Some(cell(2, 3)));
    key(&mut ui, Key::Tab, false);
    assert_eq!(ui.selection.focus, Some(cell(2, 4)));

    key(&mut ui, Key::ArrowLeft, true);
    assert_eq!(ui.selection.focus, Some(cell(2, 3)));
    assert_eq!(ui.selection.anchor, Some(cell(2, 4)));
    assert_eq!(
        ui.selection.range(),
        Some(CellRange::new(cell(2, 3), cell(2, 4)).unwrap())
    );

    ui.select_cell(cell(2, 0)).unwrap();
    key(&mut ui, Key::ArrowRight, true);
    assert_eq!(ui.selection.focus, Some(cell(2, 3)));
    assert_eq!(ui.selection.anchor, Some(cell(2, 0)));
    assert_eq!(
        ui.selection.range(),
        Some(CellRange::new(cell(2, 0), cell(2, 3)).unwrap())
    );
}

#[test]
fn explicit_selection_of_hidden_column_rejects_without_partial_change() {
    let mut ui = small_ui(6, 6);
    ui.select_cell(cell(2, 3)).unwrap();
    let selection = ui.selection.clone();
    let scroll = ui.scroll_offset();
    ui.apply_hidden_columns(&[ColumnIndex(1), ColumnIndex(4)])
        .unwrap();

    assert!(ui.select_cell(cell(2, 4)).is_err());
    assert!(ui.select_range(cell(0, 0), cell(3, 4)).is_err());
    assert_eq!(ui.selection, selection);
    assert_eq!(ui.scroll_offset(), scroll);
}

#[test]
fn restored_hidden_column_focus_moves_inside_range_and_all_hidden_columns_clear_selection() {
    let mut ui = small_ui(6, 7);
    ui.apply_hidden_columns(&[ColumnIndex(2)]).unwrap();
    let selection = SelectionState {
        kind: SelectionKind::Range,
        anchor: Some(cell(1, 1)),
        focus: Some(cell(3, 2)),
        extent: Some(CellRange::new(cell(1, 1), cell(3, 4)).unwrap()),
    };
    ui.restore_selection(selection).unwrap();
    assert_eq!(ui.selection.focus, Some(cell(3, 3)));
    assert_eq!(
        ui.selection.range(),
        Some(CellRange::new(cell(1, 1), cell(3, 4)).unwrap())
    );

    ui.apply_hidden_columns(&(0..7).map(ColumnIndex).collect::<Vec<_>>())
        .unwrap();
    let frame = ui.frame().unwrap();
    assert!(ui.selection.focus.is_none());
    assert!(frame.cells.is_empty());
    assert!(frame.column_headers.is_empty());
    assert!(
        ui.hit_test_grid(Point::new(
            ui.layout.grid_viewport.x + 1.0,
            ui.layout.grid_viewport.y + 1.0
        ))
        .is_none()
    );
    key(&mut ui, Key::ArrowRight, false);
    key(&mut ui, Key::Tab, false);
    assert_eq!(ui.selection(), &SelectionState::default());
}

#[test]
fn hidden_cell_data_and_frozen_split_survive_visibility_projection() {
    let mut ui = SheetsUi::new(UiConfig {
        dimensions: GridDimensions {
            rows: 8,
            columns: 8,
        },
        outer_width: 700,
        outer_height: 500,
        default_row_height: 20,
        default_column_width: 40,
        frozen: FrozenPanes::with(2, 3),
        ..UiConfig::default()
    })
    .unwrap();
    let address = cell(1, 2);
    assert_eq!(
        ui.grid
            .insert_cell(
                address,
                UiCell {
                    address,
                    display: "kept".into(),
                    raw: "kept".into(),
                    format: None,
                },
            )
            .unwrap(),
        None
    );
    ui.apply_hidden_columns(&[ColumnIndex(2)]).unwrap();

    assert_eq!(ui.frozen_panes(), FrozenPanes::with(2, 3));
    assert_eq!(
        ui.grid.cell(address).map(|cell| cell.raw.as_str()),
        Some("kept")
    );
    assert!(
        !ui.frame()
            .unwrap()
            .column_headers
            .iter()
            .any(|header| header.index == 2)
    );
}

#[test]
fn frozen_pane_getter_clamps_the_ui_owned_split_to_grid_dimensions() {
    let ui = SheetsUi::new(UiConfig {
        dimensions: GridDimensions {
            rows: 4,
            columns: 2,
        },
        frozen: FrozenPanes::with(20, 10),
        ..UiConfig::default()
    })
    .unwrap();
    assert_eq!(ui.frozen_panes(), FrozenPanes::with(4, 2));
}

#[test]
fn same_sheet_resize_prunes_hidden_columns_and_discards_out_of_bounds_selection() {
    let mut ui = small_ui(8, 8);
    ui.select_cell(cell(6, 6)).unwrap();
    ui.apply_hidden_columns(&[ColumnIndex(2), ColumnIndex(6)])
        .unwrap();

    let mut snapshot = ui.snapshot.clone();
    snapshot.dimensions = GridDimensions {
        rows: 4,
        columns: 4,
    };
    ui.sync(snapshot).unwrap();

    assert_eq!(ui.selection(), &SelectionState::default());
    assert!(ui.is_column_hidden(ColumnIndex(2)));
    assert!(!ui.is_column_hidden(ColumnIndex(6)));
    assert!(
        ui.frame()
            .unwrap()
            .column_headers
            .iter()
            .all(|header| header.index < 4)
    );

    let mut switched_sheet = ui.snapshot.clone();
    switched_sheet.active_sheet = SheetId(1);
    ui.sync(switched_sheet).unwrap();
    assert!(!ui.is_column_hidden(ColumnIndex(2)));
}
