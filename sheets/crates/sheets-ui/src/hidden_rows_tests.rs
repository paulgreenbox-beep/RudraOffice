use super::*;

fn cell(row: u32, column: u32) -> CellAddress {
    CellAddress::new(SheetId(0), RowIndex(row), ColumnIndex(column))
}

fn small_ui(rows: u32, columns: u32) -> SheetsUi {
    SheetsUi::new(UiConfig {
        dimensions: GridDimensions { rows, columns },
        outer_width: 500,
        outer_height: 600,
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

fn cell_rect(ui: &SheetsUi, address: CellAddress) -> Rect {
    ui.frame()
        .unwrap()
        .cells
        .into_iter()
        .find(|rendered| rendered.address == address)
        .expect("cell is rendered")
        .rect
}

fn point_for_cell(ui: &SheetsUi, address: CellAddress) -> Point {
    let frame = ui.frame().unwrap();
    let rect = frame
        .cells
        .iter()
        .find(|rendered| rendered.address == address)
        .expect("cell is rendered")
        .visible_rect
        .expect("cell intersects its pane");
    Point::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

fn pointer_down(ui: &mut SheetsUi, address: CellAddress) {
    ui.handle_event(UiEvent::PointerDown {
        point: point_for_cell(ui, address),
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    })
    .unwrap();
}

fn height_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[test]
fn hidden_mask_replaces_atomically_deduplicates_and_is_queryable() {
    let mut ui = small_ui(5, 3);
    ui.apply_hidden_rows(&[RowIndex(1), RowIndex(1), RowIndex(3)])
        .unwrap();
    assert!(ui.is_row_hidden(RowIndex(1)));
    assert!(ui.is_row_hidden(RowIndex(3)));
    assert!(!ui.is_row_hidden(RowIndex(2)));

    assert!(ui.apply_hidden_rows(&[RowIndex(2), RowIndex(5)]).is_err());
    assert!(ui.is_row_hidden(RowIndex(1)));
    assert!(ui.is_row_hidden(RowIndex(3)));
    assert!(!ui.is_row_hidden(RowIndex(2)));

    ui.apply_hidden_rows(&[RowIndex(2), RowIndex(2)]).unwrap();
    assert!(!ui.is_row_hidden(RowIndex(1)));
    assert!(ui.is_row_hidden(RowIndex(2)));
    assert!(!ui.is_row_hidden(RowIndex(3)));
}

#[test]
fn hidden_goto_endpoints_reject_without_mutating_selection_scroll_focus_or_drag() {
    let mut ui = small_ui(12, 4);
    ui.apply_hidden_rows(&[RowIndex(5)]).unwrap();
    ui.select_cell(cell(2, 1)).unwrap();
    ui.scroll = ScrollOffset { x: 17, y: 83 };
    ui.set_focus_target(FocusTarget::NameBox);
    ui.dragging = true;
    let selection = ui.selection.clone();
    let scroll = ui.scroll;
    let focus_target = ui.focus;

    assert!(ui.select_cell(cell(5, 1)).is_err());
    assert_eq!(ui.selection, selection);
    assert_eq!(ui.scroll, scroll);
    assert_eq!(ui.focus, focus_target);
    assert!(ui.dragging);

    assert!(ui.select_range(cell(1, 0), cell(5, 2)).is_err());
    assert_eq!(ui.selection, selection);
    assert_eq!(ui.scroll, scroll);
    assert_eq!(ui.focus, focus_target);
    assert!(ui.dragging);
}

#[test]
fn ranges_and_shift_navigation_keep_hidden_middle_rows_in_the_extent() {
    let mut ui = small_ui(9, 4);
    ui.apply_hidden_rows(&[RowIndex(2), RowIndex(3)]).unwrap();
    ui.select_range(cell(1, 0), cell(5, 2)).unwrap();
    let extent = CellRange::new(cell(1, 0), cell(5, 2)).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(1, 0)));
    assert_eq!(ui.selection().focus, Some(cell(5, 2)));
    assert_eq!(ui.selection().extent, Some(extent.clone()));

    ui.select_cell(cell(1, 1)).unwrap();
    key(&mut ui, Key::ArrowDown, true);
    assert_eq!(ui.selection().anchor, Some(cell(1, 1)));
    assert_eq!(ui.selection().focus, Some(cell(4, 1)));
    assert_eq!(
        ui.selection().extent,
        Some(CellRange::new(cell(1, 1), cell(4, 1)).unwrap())
    );
}

#[test]
fn vertical_navigation_skips_hidden_rows_while_tab_stays_on_the_visible_row() {
    let mut ui = small_ui(9, 4);
    ui.apply_hidden_rows(&[RowIndex(2), RowIndex(3)]).unwrap();
    ui.select_cell(cell(1, 1)).unwrap();

    key(&mut ui, Key::ArrowDown, false);
    assert_eq!(ui.selection().focus, Some(cell(4, 1)));
    key(&mut ui, Key::ArrowUp, false);
    assert_eq!(ui.selection().focus, Some(cell(1, 1)));
    key(&mut ui, Key::Enter, false);
    assert_eq!(ui.selection().focus, Some(cell(4, 1)));
    key(&mut ui, Key::Tab, false);
    assert_eq!(ui.selection().focus, Some(cell(4, 2)));
}

#[test]
fn hiding_collapsed_focus_moves_to_next_row_or_previous_at_the_bottom() {
    let mut ui = small_ui(8, 3);
    ui.select_cell(cell(3, 1)).unwrap();
    ui.apply_hidden_rows(&[RowIndex(3)]).unwrap();
    assert_eq!(ui.selection().focus, Some(cell(4, 1)));
    assert_eq!(ui.selection().anchor, Some(cell(4, 1)));
    assert_eq!(ui.selection().kind, SelectionKind::Cell);

    ui.select_cell(cell(7, 1)).unwrap();
    ui.apply_hidden_rows(&[RowIndex(3), RowIndex(7)]).unwrap();
    assert_eq!(ui.selection().focus, Some(cell(6, 1)));
    assert_eq!(ui.selection().anchor, Some(cell(6, 1)));
}

#[test]
fn range_filtering_keeps_logical_bounds_and_relocated_focus_grows_only_when_needed() {
    let mut ui = small_ui(10, 3);
    ui.select_range(cell(1, 0), cell(5, 0)).unwrap();
    let original_extent = CellRange::new(cell(1, 0), cell(5, 0)).unwrap();
    ui.apply_hidden_rows(&[RowIndex(2), RowIndex(3)]).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(1, 0)));
    assert_eq!(ui.selection().focus, Some(cell(5, 0)));
    assert_eq!(ui.selection().extent, Some(original_extent));

    ui.apply_hidden_rows(&[RowIndex(2), RowIndex(3), RowIndex(5)])
        .unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(1, 0)));
    assert_eq!(ui.selection().focus, Some(cell(4, 0)));
    assert_eq!(
        ui.selection().extent,
        Some(CellRange::new(cell(1, 0), cell(5, 0)).unwrap())
    );

    let reverse_range = CellRange::new(cell(1, 0), cell(4, 0)).unwrap();
    ui.restore_selection(SelectionState {
        kind: SelectionKind::Range,
        anchor: Some(cell(4, 0)),
        focus: Some(cell(1, 0)),
        extent: Some(reverse_range.clone()),
    })
    .unwrap();
    ui.apply_hidden_rows(&[RowIndex(1), RowIndex(2), RowIndex(3)])
        .unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(4, 0)));
    assert_eq!(ui.selection().focus, Some(cell(4, 0)));
    assert_eq!(ui.selection().extent, Some(reverse_range));
    assert_eq!(ui.selection().kind, SelectionKind::Range);
}

#[test]
fn restore_selection_validates_then_normalizes_a_hidden_focus() {
    let mut ui = small_ui(9, 4);
    ui.apply_hidden_rows(&[RowIndex(3)]).unwrap();
    let selection = SelectionState {
        kind: SelectionKind::Range,
        anchor: Some(cell(1, 1)),
        focus: Some(cell(3, 2)),
        extent: Some(CellRange::new(cell(1, 1), cell(3, 2)).unwrap()),
    };
    ui.restore_selection(selection).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(1, 1)));
    assert_eq!(ui.selection().focus, Some(cell(2, 2)));
    assert_eq!(
        ui.selection().extent,
        Some(CellRange::new(cell(1, 1), cell(3, 2)).unwrap())
    );
}

#[test]
fn wholly_hidden_range_uses_outside_fallback_and_expands_to_include_focus() {
    let mut ui = small_ui(8, 3);
    ui.select_range(cell(1, 0), cell(3, 0)).unwrap();
    ui.apply_hidden_rows(&[RowIndex(1), RowIndex(2), RowIndex(3)])
        .unwrap();

    assert_eq!(ui.selection().anchor, Some(cell(1, 0)));
    assert_eq!(ui.selection().focus, Some(cell(4, 0)));
    assert_eq!(
        ui.selection().extent,
        Some(CellRange::new(cell(1, 0), cell(4, 0)).unwrap())
    );
}

#[test]
fn frame_omits_hidden_rows_and_compresses_variable_heights_and_zoom() {
    let mut ui = small_ui(5, 2);
    ui.apply_sheet_metadata(
        &[
            RowDimension {
                row: RowIndex(1),
                height: 40.0,
            },
            RowDimension {
                row: RowIndex(2),
                height: 60.0,
            },
            RowDimension {
                row: RowIndex(3),
                height: 30.0,
            },
        ],
        &[],
        &[],
    )
    .unwrap();
    ui.apply_hidden_rows(&[RowIndex(1), RowIndex(3)]).unwrap();
    ui.set_zoom(1.5);

    let frame = ui.frame().unwrap();
    assert_eq!(
        frame
            .row_headers
            .iter()
            .map(|header| header.index)
            .collect::<Vec<_>>(),
        vec![0, 2, 4]
    );
    assert!(
        frame
            .cells
            .iter()
            .all(|rendered| rendered.address.row != RowIndex(1)
                && rendered.address.row != RowIndex(3))
    );
    let origin_y = frame.layout.grid_viewport.y;
    let row0 = cell_rect(&ui, cell(0, 0));
    let row2 = cell_rect(&ui, cell(2, 0));
    let row4 = cell_rect(&ui, cell(4, 0));
    height_close(row0.y, origin_y);
    height_close(row2.y, origin_y + 30.0);
    height_close(row2.h, 90.0);
    height_close(row4.y, origin_y + 120.0);
    height_close(row4.h, 30.0);
    height_close(frame.content_height, 150.0);
}

#[test]
fn mouse_hit_testing_maps_compressed_pixels_back_to_canonical_rows() {
    let mut ui = small_ui(8, 3);
    ui.apply_hidden_rows(&[RowIndex(1), RowIndex(3)]).unwrap();
    let target = cell(4, 1);
    let point = point_for_cell(&ui, target);
    assert_eq!(ui.hit_test_grid(point), Some(target));

    ui.handle_event(UiEvent::PointerDown {
        point,
        button: PointerButton::Primary,
        modifiers: Modifiers::default(),
    })
    .unwrap();
    assert_eq!(ui.selection().focus, Some(target));
}

#[test]
fn wheel_resize_and_height_shrink_preserve_mask_and_clamp_scroll() {
    let mut ui = SheetsUi::new(UiConfig {
        dimensions: GridDimensions {
            rows: 100,
            columns: 4,
        },
        outer_width: 500,
        outer_height: 220,
        default_row_height: 20,
        default_column_width: 40,
        ..UiConfig::default()
    })
    .unwrap();
    ui.apply_hidden_rows(&[RowIndex(3)]).unwrap();
    ui.handle_event(UiEvent::Wheel {
        delta_x: 0,
        delta_y: 50,
    })
    .unwrap();
    assert!(ui.is_row_hidden(RowIndex(3)));
    ui.handle_event(UiEvent::Resize {
        width: 700,
        height: 400,
    })
    .unwrap();
    assert!(ui.is_row_hidden(RowIndex(3)));
    ui.handle_event(UiEvent::Wheel {
        delta_x: 0,
        delta_y: 10_000,
    })
    .unwrap();

    let hidden: Vec<_> = (0..99).map(RowIndex).collect();
    ui.apply_hidden_rows(&hidden).unwrap();
    assert!(ui.is_row_hidden(RowIndex(3)));
    let frame = ui.frame().unwrap();
    assert_eq!(ui.scroll_offset().y, 0);
    assert!(frame.content_height <= frame.layout.grid_viewport.h);
}

#[test]
fn sync_preserves_same_sheet_mask_and_clears_it_for_a_different_sheet() {
    let mut ui = small_ui(8, 4);
    ui.apply_hidden_rows(&[RowIndex(2), RowIndex(6)]).unwrap();
    let same_sheet = ui.snapshot.clone();
    ui.sync(same_sheet).unwrap();
    assert!(ui.is_row_hidden(RowIndex(2)));
    assert!(ui.is_row_hidden(RowIndex(6)));

    let mut other_sheet = ui.snapshot.clone();
    other_sheet.active_sheet = SheetId(1);
    ui.sync(other_sheet).unwrap();
    assert!(!ui.is_row_hidden(RowIndex(2)));
    assert!(!ui.is_row_hidden(RowIndex(6)));
}

#[test]
fn all_hidden_and_empty_grids_have_no_targets_and_navigation_is_safe() {
    let mut ui = small_ui(4, 3);
    ui.select_cell(cell(2, 1)).unwrap();
    ui.apply_hidden_rows(&[RowIndex(0), RowIndex(1), RowIndex(2), RowIndex(3)])
        .unwrap();
    assert_eq!(ui.selection(), &SelectionState::default());
    let frame = ui.frame().unwrap();
    assert!(frame.cells.is_empty());
    assert!(frame.row_headers.is_empty());
    let viewport = frame.layout.grid_viewport;
    let point = Point::new(viewport.x + viewport.w / 2.0, viewport.y + viewport.h / 2.0);
    assert_eq!(ui.hit_test_grid(point), None);
    for movement in [Key::ArrowDown, Key::Enter, Key::Tab] {
        key(&mut ui, movement, false);
    }
    assert_eq!(ui.selection(), &SelectionState::default());

    let mut empty = SheetsUi::new(UiConfig {
        dimensions: GridDimensions {
            rows: 0,
            columns: 0,
        },
        ..UiConfig::default()
    })
    .unwrap();
    for movement in [Key::ArrowDown, Key::Enter, Key::Tab] {
        key(&mut empty, movement, false);
    }
    assert_eq!(empty.selection(), &SelectionState::default());
    assert!(empty.frame().unwrap().cells.is_empty());
}

#[test]
fn sparse_cells_and_row_dimensions_survive_hiding_and_unhiding() {
    let mut ui = small_ui(6, 3);
    let hidden_cell = cell(2, 1);
    let visible_cell = cell(4, 2);
    let mut snapshot = ui.snapshot.clone();
    snapshot.cells = vec![
        UiCell {
            address: hidden_cell,
            display: "hidden".into(),
            raw: "hidden".into(),
            format: None,
        },
        UiCell {
            address: visible_cell,
            display: "visible".into(),
            raw: "visible".into(),
            format: None,
        },
    ];
    ui.sync(snapshot).unwrap();
    let original_cells = ui.snapshot.cells.clone();
    ui.apply_sheet_metadata(
        &[RowDimension {
            row: RowIndex(2),
            height: 48.0,
        }],
        &[],
        &[],
    )
    .unwrap();
    ui.apply_hidden_rows(&[RowIndex(2)]).unwrap();
    assert_eq!(ui.snapshot.cells, original_cells);
    assert!(ui.grid.cell(hidden_cell).is_some());
    assert!(
        !ui.frame()
            .unwrap()
            .cells
            .iter()
            .any(|rendered| rendered.address == hidden_cell)
    );

    ui.apply_sheet_metadata(
        &[RowDimension {
            row: RowIndex(2),
            height: 72.0,
        }],
        &[],
        &[],
    )
    .unwrap();
    ui.apply_hidden_rows(&[]).unwrap();
    let row2 = cell_rect(&ui, cell(2, 0));
    let row3 = cell_rect(&ui, cell(3, 0));
    height_close(row2.h, 72.0);
    height_close(row3.y - row2.y, 72.0);
    assert_eq!(ui.grid.cell(hidden_cell).unwrap().display, "hidden");
}

#[test]
fn applying_the_same_mask_keeps_drag_active_but_a_changed_mask_cancels_it() {
    let mut ui = small_ui(100, 3);
    ui.apply_hidden_rows(&[RowIndex(5)]).unwrap();
    pointer_down(&mut ui, cell(2, 0));
    ui.scroll = ScrollOffset { x: 0, y: 400 };
    let selection = ui.selection.clone();
    assert!(ui.dragging);

    ui.apply_hidden_rows(&[RowIndex(5), RowIndex(5)]).unwrap();
    assert!(ui.dragging);
    assert_eq!(ui.selection, selection);
    assert_eq!(ui.scroll, ScrollOffset { x: 0, y: 400 });

    let changed_mask: Vec<_> = (0..99).map(RowIndex).collect();
    ui.apply_hidden_rows(&changed_mask).unwrap();
    assert!(!ui.dragging);
    assert_eq!(ui.selection().anchor, Some(cell(99, 0)));
    assert_eq!(ui.selection().focus, Some(cell(99, 0)));
    assert_eq!(ui.scroll_offset().y, 0);
}

#[test]
fn merge_projection_omits_fully_hidden_merges_and_compresses_partial_merges() {
    let mut ui = small_ui(8, 4);
    let partial = CellRange::new(cell(0, 1), cell(3, 1)).unwrap();
    let fully_hidden = CellRange::new(cell(5, 2), cell(6, 2)).unwrap();
    ui.apply_sheet_metadata(&[], &[], &[partial.clone(), fully_hidden.clone()])
        .unwrap();
    ui.apply_hidden_rows(&[RowIndex(1), RowIndex(2), RowIndex(5), RowIndex(6)])
        .unwrap();
    ui.set_zoom(1.5);

    let frame = ui.frame().unwrap();
    assert_eq!(frame.merges, vec![partial.clone(), fully_hidden]);
    assert_eq!(frame.merged_cells.len(), 1);
    assert_eq!(frame.merged_cells[0].range, partial);
    height_close(frame.merged_cells[0].rect.h, 60.0);
}

#[test]
fn frozen_panes_and_zoom_use_compressed_row_offsets() {
    let mut ui = SheetsUi::new(UiConfig {
        dimensions: GridDimensions {
            rows: 8,
            columns: 4,
        },
        outer_width: 500,
        outer_height: 260,
        default_row_height: 20,
        default_column_width: 40,
        frozen: FrozenPanes::with(1, 0),
        ..UiConfig::default()
    })
    .unwrap();
    ui.apply_hidden_rows(&[RowIndex(1), RowIndex(4)]).unwrap();
    ui.set_zoom(2.0);
    ui.scroll = ScrollOffset { x: 0, y: 20 };

    let frame = ui.frame().unwrap();
    assert!(frame.row_headers.iter().any(|header| header.index == 0));
    assert!(
        frame
            .row_headers
            .iter()
            .all(|header| header.index != 1 && header.index != 4)
    );
    height_close(frame.content_height, 240.0);
    height_close(frame.scroll_y, 40.0);
    let row3 = cell_rect(&ui, cell(3, 0));
    height_close(row3.y, frame.layout.grid_viewport.y + 40.0);
}

#[test]
fn hidden_rows_do_not_change_endpoint_intent_or_whole_merge_selection_closure() {
    let mut ui = small_ui(8, 5);
    let merge = CellRange::new(cell(2, 1), cell(4, 2)).unwrap();
    ui.apply_sheet_metadata(&[], &[], std::slice::from_ref(&merge))
        .unwrap();
    ui.select_range(cell(0, 1), cell(2, 1)).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(0, 1)));
    assert_eq!(ui.selection().focus, Some(cell(2, 1)));
    assert_eq!(
        ui.selection().extent,
        Some(CellRange::new(cell(0, 1), cell(4, 2)).unwrap())
    );

    ui.apply_hidden_rows(&[RowIndex(3)]).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(0, 1)));
    assert_eq!(ui.selection().focus, Some(cell(2, 1)));
    assert_eq!(
        ui.selection().extent,
        Some(CellRange::new(cell(0, 1), cell(4, 2)).unwrap())
    );
}

#[test]
fn hidden_range_focus_prefers_visible_rows_inside_extent_and_metadata_keeps_bounds() {
    let mut ui = small_ui(8, 3);
    ui.select_range(cell(0, 0), cell(5, 1)).unwrap();
    let full_range = CellRange::new(cell(0, 0), cell(5, 1)).unwrap();
    ui.apply_hidden_rows(&[RowIndex(4), RowIndex(5)]).unwrap();

    assert_eq!(ui.selection().anchor, Some(cell(0, 0)));
    assert_eq!(ui.selection().focus, Some(cell(3, 1)));
    assert_eq!(ui.selection().extent, Some(full_range.clone()));

    ui.apply_sheet_metadata(&[], &[], &[]).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(0, 0)));
    assert_eq!(ui.selection().focus, Some(cell(3, 1)));
    assert_eq!(ui.selection().extent, Some(full_range));
}

#[test]
fn hiding_a_merged_single_cell_endpoint_preserves_merge_extent_until_unmerge() {
    let mut ui = small_ui(8, 3);
    let merge = CellRange::new(cell(2, 0), cell(4, 0)).unwrap();
    ui.apply_sheet_metadata(&[], &[], std::slice::from_ref(&merge))
        .unwrap();
    ui.select_cell(cell(2, 0)).unwrap();
    ui.apply_hidden_rows(&[RowIndex(2)]).unwrap();

    assert_eq!(ui.selection().anchor, Some(cell(3, 0)));
    assert_eq!(ui.selection().focus, Some(cell(3, 0)));
    assert_eq!(ui.selection().extent, Some(merge));

    ui.apply_sheet_metadata(&[], &[], &[]).unwrap();
    let single_cell = CellRange::new(cell(3, 0), cell(3, 0)).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(3, 0)));
    assert_eq!(ui.selection().focus, Some(cell(3, 0)));
    assert_eq!(ui.selection().extent, Some(single_cell));
    assert_eq!(ui.selection().kind, SelectionKind::Cell);
}

#[test]
fn unmerging_recomputes_merge_expanded_range_from_relocated_endpoints_and_format_effect() {
    let mut ui = small_ui(12, 3);
    let merge = CellRange::new(cell(1, 0), cell(4, 0)).unwrap();
    ui.apply_sheet_metadata(&[], &[], std::slice::from_ref(&merge))
        .unwrap();
    ui.select_range(cell(0, 0), cell(2, 0)).unwrap();
    assert_eq!(
        ui.selection().extent,
        Some(CellRange::new(cell(0, 0), cell(4, 0)).unwrap())
    );

    ui.apply_hidden_rows(&[RowIndex(2)]).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(0, 0)));
    assert_eq!(ui.selection().focus, Some(cell(3, 0)));
    assert_eq!(
        ui.selection().extent,
        Some(CellRange::new(cell(0, 0), cell(4, 0)).unwrap())
    );

    ui.apply_sheet_metadata(&[], &[], &[]).unwrap();
    let current_range = CellRange::new(cell(0, 0), cell(3, 0)).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(0, 0)));
    assert_eq!(ui.selection().focus, Some(cell(3, 0)));
    assert_eq!(ui.selection().extent, Some(current_range.clone()));
    assert_eq!(
        ui.handle_event(UiEvent::ApplyFormat(FormatPatch::default()))
            .unwrap(),
        vec![UiEffect::ApplyFormat {
            range: current_range,
            patch: FormatPatch::default(),
        }]
    );
}

#[test]
fn removing_unrelated_merge_keeps_explicit_range_bounds_after_hidden_focus_moves() {
    let mut ui = small_ui(12, 4);
    let unrelated_merge = CellRange::new(cell(8, 2), cell(9, 2)).unwrap();
    ui.apply_sheet_metadata(&[], &[], std::slice::from_ref(&unrelated_merge))
        .unwrap();
    ui.select_range(cell(0, 0), cell(5, 1)).unwrap();
    let explicit_range = CellRange::new(cell(0, 0), cell(5, 1)).unwrap();
    ui.apply_hidden_rows(&[RowIndex(5)]).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(0, 0)));
    assert_eq!(ui.selection().focus, Some(cell(4, 1)));
    assert_eq!(ui.selection().extent, Some(explicit_range.clone()));

    ui.apply_sheet_metadata(&[], &[], &[]).unwrap();
    assert_eq!(ui.selection().anchor, Some(cell(0, 0)));
    assert_eq!(ui.selection().focus, Some(cell(4, 1)));
    assert_eq!(ui.selection().extent, Some(explicit_range));
}
