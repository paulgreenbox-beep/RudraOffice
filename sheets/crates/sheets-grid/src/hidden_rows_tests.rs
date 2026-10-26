use super::*;

fn make_grid(rows: u32, columns: u32) -> VirtualGrid<String> {
    VirtualGrid::new(SheetId(0), GridDimensions { rows, columns }, 20, 12).unwrap()
}

#[test]
fn hidden_overrides_keep_positive_sizes_and_mask_replacement_is_atomic() {
    let mut axis = VariableAxis::new(10, 1, 100).unwrap();
    axis.set_override(2, 20, 6).unwrap();
    axis.set_hidden(&[2, 4, 2], 6).unwrap();

    assert!(axis.is_hidden(2));
    assert!(axis.is_hidden(4));
    assert_eq!(axis.get(2), Some(20));
    assert_eq!(axis.size(2), 0);
    assert_eq!(axis.total_extent_exact(6).unwrap(), 40);

    // Repeating the same mask in a different order is idempotent.
    axis.set_hidden(&[4, 2], 6).unwrap();
    assert_eq!(axis.total_extent_exact(6).unwrap(), 40);
    assert_eq!(
        axis.visible_indices(IndexRange { start: 0, end: 5 }, 6)
            .collect::<Vec<_>>(),
        vec![0, 1, 3, 5]
    );

    // A valid prefix followed by an invalid index must leave the old mask intact.
    assert!(matches!(
        axis.set_hidden(&[1, 6], 6),
        Err(GridError::OutOfBounds { .. })
    ));
    assert!(axis.is_hidden(2));
    assert!(axis.is_hidden(4));
    assert!(!axis.is_hidden(1));
    assert_eq!(axis.total_extent_exact(6).unwrap(), 40);

    // Hidden dimensions remain configured, and can be changed while hidden.
    axis.set_override(2, 30, 6).unwrap();
    assert_eq!(axis.get(2), Some(30));
    assert_eq!(axis.size(2), 0);
    assert_eq!(axis.total_extent_exact(6).unwrap(), 40);
    assert!(matches!(
        axis.set_override(4, 0, 6),
        Err(GridError::InvalidAxisSize { value: 0, .. })
    ));
    assert_eq!(axis.get(4), None);

    // Revealing the changed row applies its retained override to the extent.
    axis.set_hidden(&[4], 6).unwrap();
    assert_eq!(axis.size(2), 30);
    assert_eq!(axis.total_extent_exact(6).unwrap(), 70);
    assert_eq!(axis.remove_override(2, 6).unwrap(), Some(30));
    assert_eq!(axis.total_extent_exact(6).unwrap(), 50);
    axis.set_hidden(&[], 6).unwrap();
    assert_eq!(axis.total_extent_exact(6).unwrap(), 60);
}

#[test]
fn sparse_axis_prefixes_and_inverse_offsets_match_a_dense_oracle_across_blocks() {
    let axis_len = 530;
    let mut axis = VariableAxis::new(7, 1, 100).unwrap();
    let overrides = [
        (254, 13),
        (255, 20),
        (256, 3),
        (257, 11),
        (510, 15),
        (511, 6),
        (512, 8),
    ];
    let hidden = [255, 256, 257, 511];
    let mut expected = vec![7; axis_len as usize];

    for (index, size) in overrides {
        axis.set_override(index, size, axis_len).unwrap();
        expected[index as usize] = size;
    }
    axis.set_hidden(&hidden, axis_len).unwrap();
    for index in hidden {
        expected[index as usize] = 0;
    }

    for index in 0..=axis_len {
        let expected_prefix = expected[..index as usize].iter().copied().sum::<u32>();
        assert_eq!(
            axis.offset_for_index(index, axis_len).unwrap(),
            expected_prefix,
            "prefix at {index}"
        );
    }

    let total = expected.iter().copied().sum::<u32>();
    assert_eq!(
        axis.total_extent_exact(axis_len).unwrap(),
        u128::from(total)
    );
    let mut start = 0;
    for (index, size) in expected.iter().copied().enumerate() {
        if size != 0 {
            assert_eq!(
                axis.offset_to_index(u64::from(start), axis_len).unwrap(),
                index as u32
            );
            assert_eq!(
                axis.offset_to_index(u64::from(start + size - 1), axis_len)
                    .unwrap(),
                index as u32
            );
        }
        start += size;
    }
    assert!(axis.offset_to_index(u64::from(total), axis_len).is_err());
}

#[test]
fn fully_hidden_axes_and_zero_extent_blocks_have_no_phantom_hits() {
    let mut all_hidden = VariableAxis::new(10, 1, 100).unwrap();
    all_hidden.set_hidden(&[0, 1, 2, 3], 4).unwrap();
    assert_eq!(all_hidden.total_extent_exact(4).unwrap(), 0);
    assert_eq!(all_hidden.next_visible(0, 4), None);
    assert_eq!(all_hidden.previous_visible(3, 4), None);
    assert!(
        all_hidden
            .visible_indices(IndexRange { start: 0, end: 3 }, 4)
            .next()
            .is_none()
    );
    assert_eq!(all_hidden.visible_interval(0, 4, 0, 40, 2).unwrap(), None);
    assert!(all_hidden.offset_to_index(0, 4).is_err());

    // A complete zero-height block is skipped when locating the first pixel.
    let mut zero_block = VariableAxis::new(10, 1, 100).unwrap();
    let hidden_block = (0..256).collect::<Vec<_>>();
    zero_block.set_hidden(&hidden_block, 257).unwrap();
    assert_eq!(zero_block.total_extent_exact(257).unwrap(), 10);
    assert_eq!(zero_block.offset_for_index(256, 257).unwrap(), 0);
    assert_eq!(zero_block.offset_to_index(0, 257).unwrap(), 256);
    assert_eq!(zero_block.offset_to_index(9, 257).unwrap(), 256);
    assert!(zero_block.offset_to_index(10, 257).is_err());
}

#[test]
fn hidden_rows_have_zero_geometry_and_hit_testing_jumps_over_them() {
    let mut grid = make_grid(6, 1);
    grid.set_row_size(RowIndex(2), 30).unwrap();
    grid.set_hidden_rows(&[RowIndex(1), RowIndex(2)]).unwrap();

    let hidden_rect = grid
        .cell_rect(CellAddress::new(SheetId(0), RowIndex(2), ColumnIndex(0)))
        .unwrap();
    assert_eq!(hidden_rect.y, 20.0);
    assert_eq!(hidden_rect.h, 0.0);
    let next_rect = grid
        .cell_rect(CellAddress::new(SheetId(0), RowIndex(3), ColumnIndex(0)))
        .unwrap();
    assert_eq!(next_rect.y, 20.0);
    assert_eq!(next_rect.h, 20.0);

    let hidden_range = CellRange::new(
        CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(0)),
        CellAddress::new(SheetId(0), RowIndex(2), ColumnIndex(0)),
    )
    .unwrap();
    assert_eq!(grid.range_rect(&hidden_range).unwrap().h, 0.0);
    let visible_range = CellRange::new(
        CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(0)),
        CellAddress::new(SheetId(0), RowIndex(4), ColumnIndex(0)),
    )
    .unwrap();
    assert_eq!(grid.range_rect(&visible_range).unwrap().h, 40.0);

    let view = Viewport::new(0.0, 0.0, 12.0, 120.0);
    let hit = |y| grid.hit_test(Point::new(1.0, y), view, ScrollOffset::default());
    assert_eq!(hit(19.0).unwrap().row, RowIndex(0));
    assert_eq!(hit(20.0).unwrap().row, RowIndex(3));
    assert_eq!(hit(39.0).unwrap().row, RowIndex(3));
    assert_eq!(hit(40.0).unwrap().row, RowIndex(4));
    assert_eq!(hit(79.0).unwrap().row, RowIndex(5));
    assert!(hit(80.0).is_none());
}

#[test]
fn sparse_hidden_prefix_at_partial_block_boundary_preserves_frozen_extent() {
    let mut grid = make_grid(600, 2);
    grid.set_row_size(RowIndex(256), 35).unwrap();
    grid.set_row_size(RowIndex(259), 5).unwrap();
    grid.set_hidden_rows(&[RowIndex(258)]).unwrap();
    grid.set_frozen_panes(FrozenPanes::with(260, 0));

    let frozen_rows = grid.frozen.clamp_to_dimensions(&grid.dimensions).rows;
    let frozen_extent = grid.row_sizes.total_extent_exact(frozen_rows).unwrap();
    assert_eq!(frozen_extent, 5_180);
    let viewport = Viewport::new(0.0, 0.0, 24.0, 6_000.0);
    let before = grid.layout(viewport, ScrollOffset::default());
    let content_before = before.content_height;

    // These changes share nearby sparse blocks but are beyond the frozen prefix.
    grid.set_hidden_rows(&[RowIndex(258), RowIndex(511)])
        .unwrap();
    grid.set_row_size(RowIndex(512), 80).unwrap();

    assert_eq!(
        grid.row_sizes.total_extent_exact(frozen_rows).unwrap(),
        frozen_extent
    );
    let after = grid.layout(viewport, ScrollOffset::default());
    assert_ne!(after.content_height, content_before);
    assert_eq!(after.frozen_rows, before.frozen_rows);
    assert_eq!(after.body_rows, before.body_rows);
}

#[test]
fn frozen_hidden_rows_and_body_scroll_resolve_to_visible_rows() {
    let mut grid = make_grid(12, 3);
    grid.set_frozen_panes(FrozenPanes::with(2, 0));
    grid.set_hidden_rows(&[RowIndex(0)]).unwrap();
    let view = Viewport::new(100.0, 50.0, 36.0, 60.0);
    let scroll = ScrollOffset::new(0.0, 20.0);

    let layout = grid.layout(view, scroll);
    assert_eq!(layout.frozen_rows, Some(IndexRange { start: 1, end: 1 }));
    assert_eq!(layout.body_rows.unwrap().start, 3);

    let hit = |y| grid.hit_test(Point::new(101.0, y), view, scroll);
    assert_eq!(hit(50.0).unwrap().row, RowIndex(1));
    assert_eq!(hit(69.0).unwrap().row, RowIndex(1));
    assert_eq!(hit(70.0).unwrap().row, RowIndex(3));
    assert_eq!(hit(90.0).unwrap().row, RowIndex(4));
}

#[test]
fn visible_rows_jumps_over_a_long_hidden_run() {
    let mut grid = make_grid(5_000, 1);
    let hidden = (1..4_096).map(RowIndex).collect::<Vec<_>>();
    grid.set_hidden_rows(&hidden).unwrap();

    let visible = grid
        .visible_rows(IndexRange {
            start: 0,
            end: 4_100,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        vec![
            RowIndex(0),
            RowIndex(4_096),
            RowIndex(4_097),
            RowIndex(4_098),
            RowIndex(4_099),
            RowIndex(4_100),
        ]
    );
    assert_eq!(grid.hidden_rows().count(), hidden.len());
    assert_eq!(grid.next_visible_row(RowIndex(1)), Some(RowIndex(4_096)));
    assert_eq!(
        grid.previous_visible_row(RowIndex(4_095)),
        Some(RowIndex(0))
    );
}

#[test]
fn resize_prunes_hidden_rows_outside_the_new_dimensions() {
    let mut grid = make_grid(5, 2);
    grid.set_hidden_rows(&[RowIndex(1), RowIndex(4)]).unwrap();
    grid.resize(GridDimensions {
        rows: 3,
        columns: 2,
    });
    assert_eq!(grid.hidden_rows().collect::<Vec<_>>(), vec![RowIndex(1)]);
    assert!(!grid.is_row_hidden(RowIndex(4)));
    assert_eq!(grid.row_sizes.total_extent_exact(3).unwrap(), 40);

    grid.resize(GridDimensions {
        rows: 1,
        columns: 2,
    });
    assert_eq!(grid.hidden_rows().count(), 0);
    assert_eq!(grid.row_sizes.total_extent_exact(1).unwrap(), 20);
}

#[test]
fn scroll_clamping_uses_visible_row_extent() {
    let mut grid = make_grid(8, 5);
    grid.set_row_size(RowIndex(2), 35).unwrap();
    grid.set_hidden_rows(&[RowIndex(1), RowIndex(6)]).unwrap();
    let viewport = Viewport::new(0.0, 0.0, 24.0, 40.0);

    assert_eq!(
        grid.clamp_scroll(viewport, ScrollOffset::new(500.0, 500.0)),
        ScrollOffset { x: 36, y: 95 }
    );
    grid.set_hidden_rows(&[RowIndex(1), RowIndex(2), RowIndex(6)])
        .unwrap();
    assert_eq!(
        grid.clamp_scroll(viewport, ScrollOffset::new(500.0, 500.0)),
        ScrollOffset { x: 36, y: 60 }
    );
}
