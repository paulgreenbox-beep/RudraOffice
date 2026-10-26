use super::*;

fn grid(rows: u32, columns: u32) -> VirtualGrid<()> {
    VirtualGrid::new(SheetId(0), GridDimensions { rows, columns }, 20, 10).unwrap()
}

#[test]
fn hidden_columns_compress_offsets_hits_and_visible_iteration() {
    let mut grid = grid(4, 8);
    grid.set_hidden_columns(&[ColumnIndex(1), ColumnIndex(2)])
        .unwrap();

    let view = Viewport::new(0.0, 0.0, 50.0, 40.0);
    assert_eq!(grid.layout(view, ScrollOffset::default()).content_width, 60);
    assert_eq!(
        grid.visible_columns(IndexRange { start: 0, end: 5 })
            .collect::<Vec<_>>(),
        [
            ColumnIndex(0),
            ColumnIndex(3),
            ColumnIndex(4),
            ColumnIndex(5)
        ]
    );
    assert_eq!(
        grid.cell_rect(CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(3))),
        Some(Rect::new(10.0, 0.0, 10.0, 20.0))
    );
    assert_eq!(
        grid.hit_test(Point::new(11.0, 1.0), view, ScrollOffset::default()),
        Some(CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(3)))
    );
}

#[test]
fn hidden_column_mask_is_atomic_and_keeps_positive_width_overrides() {
    let mut grid = grid(3, 6);
    grid.set_column_size(ColumnIndex(2), 30).unwrap();
    grid.set_hidden_columns(&[ColumnIndex(2), ColumnIndex(4), ColumnIndex(2)])
        .unwrap();
    assert!(grid.is_column_hidden(ColumnIndex(2)));
    assert_eq!(grid.column_sizes.get(2), Some(30));
    assert_eq!(grid.column_sizes.size(2), 0);
    assert_eq!(
        grid.layout(
            Viewport::new(0.0, 0.0, 100.0, 20.0),
            ScrollOffset::default()
        )
        .content_width,
        40
    );

    assert!(
        grid.set_hidden_columns(&[ColumnIndex(1), ColumnIndex(6)])
            .is_err()
    );
    assert!(grid.is_column_hidden(ColumnIndex(2)));
    assert!(grid.is_column_hidden(ColumnIndex(4)));
    assert!(!grid.is_column_hidden(ColumnIndex(1)));

    grid.set_hidden_columns(&[ColumnIndex(4)]).unwrap();
    assert_eq!(grid.column_sizes.size(2), 30);
    assert_eq!(
        grid.layout(
            Viewport::new(0.0, 0.0, 100.0, 20.0),
            ScrollOffset::default()
        )
        .content_width,
        70
    );
}

#[test]
fn clearing_shifted_dimensions_keeps_visibility_masks_and_uses_new_sizes() {
    let mut grid = grid(4, 4);
    grid.set_row_size(RowIndex(1), 30).unwrap();
    grid.set_column_size(ColumnIndex(2), 30).unwrap();
    grid.set_hidden_rows(&[RowIndex(1)]).unwrap();
    grid.set_hidden_columns(&[ColumnIndex(2)]).unwrap();

    assert!(grid.clear_axis_overrides());
    assert!(!grid.clear_axis_overrides());
    grid.set_row_size(RowIndex(3), 25).unwrap();
    grid.set_column_size(ColumnIndex(1), 15).unwrap();
    assert_eq!(grid.row_sizes.size(1), 0);
    assert_eq!(grid.column_sizes.size(2), 0);
    assert_eq!(grid.row_sizes.get(3), Some(25));
    assert_eq!(grid.column_sizes.get(1), Some(15));
    assert_eq!(grid.row_sizes.total_extent_exact(4).unwrap(), 65);
    assert_eq!(grid.column_sizes.total_extent_exact(4).unwrap(), 35);

    grid.set_hidden_rows(&[]).unwrap();
    grid.set_hidden_columns(&[]).unwrap();
    assert_eq!(grid.row_sizes.total_extent_exact(4).unwrap(), 85);
    assert_eq!(grid.column_sizes.total_extent_exact(4).unwrap(), 45);
}

#[test]
fn frozen_hidden_columns_are_skipped_and_scroll_uses_compressed_extent() {
    let mut grid = grid(4, 8);
    grid.set_frozen_panes(FrozenPanes::with(0, 3));
    grid.set_hidden_columns(&[ColumnIndex(1), ColumnIndex(2), ColumnIndex(6)])
        .unwrap();

    let layout = grid.layout(
        Viewport::new(0.0, 0.0, 35.0, 40.0),
        ScrollOffset { x: 10, y: 0 },
    );
    assert_eq!(layout.frozen_cols, Some(IndexRange { start: 0, end: 0 }));
    assert_eq!(layout.content_width, 50);
    assert_eq!(
        grid.clamp_scroll(
            Viewport::new(0.0, 0.0, 35.0, 40.0),
            ScrollOffset { x: 100, y: 0 }
        )
        .x,
        15
    );
}

#[test]
fn frozen_hidden_columns_hit_correct_visible_cells_after_horizontal_scroll() {
    let mut grid = grid(4, 8);
    grid.set_frozen_panes(FrozenPanes::with(0, 3));
    grid.set_hidden_columns(&[ColumnIndex(1), ColumnIndex(4), ColumnIndex(6)])
        .unwrap();

    let view = Viewport::new(0.0, 0.0, 40.0, 40.0);
    let scroll = ScrollOffset { x: 10, y: 0 };
    let layout = grid.layout(view, scroll);
    assert_eq!(layout.content_width, 50);
    assert_eq!(layout.frozen_cols, Some(IndexRange { start: 0, end: 2 }));
    assert_eq!(layout.body_cols, Some(IndexRange { start: 5, end: 7 }));
    assert_eq!(
        grid.hit_test(Point::new(11.0, 1.0), view, scroll),
        Some(CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(2)))
    );
    assert_eq!(
        grid.hit_test(Point::new(21.0, 1.0), view, scroll),
        Some(CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(5)))
    );
}

#[test]
fn resizing_prunes_out_of_bounds_hidden_columns() {
    let mut grid = grid(3, 8);
    grid.set_hidden_columns(&[ColumnIndex(1), ColumnIndex(6)])
        .unwrap();
    grid.resize(GridDimensions {
        rows: 3,
        columns: 4,
    });
    assert_eq!(grid.hidden_columns().collect::<Vec<_>>(), [ColumnIndex(1)]);
    assert_eq!(
        grid.next_visible_column(ColumnIndex(1)),
        Some(ColumnIndex(2))
    );
    assert_eq!(
        grid.previous_visible_column(ColumnIndex(99)),
        Some(ColumnIndex(3))
    );
}
