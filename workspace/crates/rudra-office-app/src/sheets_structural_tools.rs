//! Pure selection-to-command mapping for structural sheet edits.

use std::fmt;

use sheets_core::{CellAddress, CellRange, ColumnIndex, RowIndex};

use crate::ribbon_catalog::RibbonCommand;

const MAX_ROWS: u32 = 1_048_576;
const MAX_COLUMNS: u32 = 16_384;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StructuralAxis {
    Rows,
    Columns,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StructuralSelection {
    pub(super) axis: StructuralAxis,
    /// Zero-based first selected row or column.
    pub(super) at: u32,
    /// Number of contiguous selected rows or columns.
    pub(super) count: u32,
    /// Post-operation range, preserving the selected perpendicular span.
    pub(super) target_range: CellRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StructuralSelectionError {
    InvalidSelection,
    UnsupportedCommand,
    OutOfBounds,
    Overflow,
    NoSurvivingTarget,
}

impl fmt::Display for StructuralSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidSelection => "Выделение строк или столбцов некорректно.",
            Self::UnsupportedCommand => {
                "Команда не является вставкой или удалением строк/столбцов."
            }
            Self::OutOfBounds => "Выделение выходит за границы листа.",
            Self::Overflow => "Размер выделения выходит за диапазон поддерживаемых индексов.",
            Self::NoSurvivingTarget => {
                "Нельзя удалить весь лист: после удаления не останется ячеек для выделения."
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for StructuralSelectionError {}

/// Maps a ribbon command and canonical cell selection to a bounded row/column edit.
///
/// The selected extent on the structural axis determines `count`, matching
/// spreadsheet-editor behavior for inserting multiple rows or columns. Deletes
/// retain the selected band size afterward: they prefer rows/columns shifted
/// into the deleted start, extend backward at the sheet edge, and clamp the
/// target to valid coordinates. The perpendicular selection extent is retained
/// rather than widened to a whole-sheet range.
pub(super) fn plan_structural_selection(
    selection: &CellRange,
    command: RibbonCommand,
) -> Result<StructuralSelection, StructuralSelectionError> {
    let axis = match command {
        RibbonCommand::InsertRows | RibbonCommand::DeleteRows => StructuralAxis::Rows,
        RibbonCommand::InsertColumns | RibbonCommand::DeleteColumns => StructuralAxis::Columns,
        _ => return Err(StructuralSelectionError::UnsupportedCommand),
    };
    let deleting = matches!(
        command,
        RibbonCommand::DeleteRows | RibbonCommand::DeleteColumns
    );

    selection
        .validate()
        .map_err(|_| StructuralSelectionError::InvalidSelection)?;

    let start = selection.start();
    let end = selection.end();
    let row_count = inclusive_count(start.row.0, end.row.0)?;
    let column_count = inclusive_count(start.column.0, end.column.0)?;

    if start.row.0 >= MAX_ROWS
        || end.row.0 >= MAX_ROWS
        || start.column.0 >= MAX_COLUMNS
        || end.column.0 >= MAX_COLUMNS
    {
        return Err(StructuralSelectionError::OutOfBounds);
    }

    let (at, count, limit) = match axis {
        StructuralAxis::Rows => (start.row.0, row_count, MAX_ROWS),
        StructuralAxis::Columns => (start.column.0, column_count, MAX_COLUMNS),
    };

    let (target_at, target_count) = if deleting {
        if count == limit {
            return Err(StructuralSelectionError::NoSurvivingTarget);
        }

        let target_count = count;
        let after_deleted = at
            .checked_add(count)
            .ok_or(StructuralSelectionError::Overflow)?;
        let following_count = limit
            .checked_sub(after_deleted)
            .ok_or(StructuralSelectionError::OutOfBounds)?;
        let following_target_count = target_count.min(following_count);
        let preceding_target_count = target_count - following_target_count;
        let max_target_at = limit
            .checked_sub(target_count)
            .ok_or(StructuralSelectionError::OutOfBounds)?;
        let target_at = at.saturating_sub(preceding_target_count).min(max_target_at);
        (target_at, target_count)
    } else {
        (at, count)
    };

    let target_end = target_at
        .checked_add(
            target_count
                .checked_sub(1)
                .ok_or(StructuralSelectionError::NoSurvivingTarget)?,
        )
        .ok_or(StructuralSelectionError::Overflow)?;
    if target_end >= limit {
        return Err(StructuralSelectionError::OutOfBounds);
    }

    let (target_start_row, target_start_column, target_end_row, target_end_column) = match axis {
        StructuralAxis::Rows => (
            RowIndex(target_at),
            start.column,
            RowIndex(target_end),
            end.column,
        ),
        StructuralAxis::Columns => (
            start.row,
            ColumnIndex(target_at),
            end.row,
            ColumnIndex(target_end),
        ),
    };
    let target_range = CellRange::new(
        CellAddress::new(selection.sheet(), target_start_row, target_start_column),
        CellAddress::new(selection.sheet(), target_end_row, target_end_column),
    )
    .map_err(|_| StructuralSelectionError::InvalidSelection)?;

    Ok(StructuralSelection {
        axis,
        at,
        count,
        target_range,
    })
}

fn inclusive_count(start: u32, end: u32) -> Result<u32, StructuralSelectionError> {
    end.checked_sub(start)
        .and_then(|span| span.checked_add(1))
        .ok_or(StructuralSelectionError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sheets_core::SheetId;

    fn range(start_row: u32, start_column: u32, end_row: u32, end_column: u32) -> CellRange {
        let sheet = SheetId(7);
        CellRange::new(
            CellAddress::new(sheet, RowIndex(start_row), ColumnIndex(start_column)),
            CellAddress::new(sheet, RowIndex(end_row), ColumnIndex(end_column)),
        )
        .expect("test range must be constructible")
    }

    #[test]
    fn insert_rows_uses_normalized_extent_and_preserves_selected_columns() {
        let selection = range(6, 4, 3, 2);
        let plan = plan_structural_selection(&selection, RibbonCommand::InsertRows).unwrap();

        assert_eq!(plan.axis, StructuralAxis::Rows);
        assert_eq!(plan.at, 3);
        assert_eq!(plan.count, 4);
        assert_eq!(plan.target_range, range(3, 2, 6, 4));
    }

    #[test]
    fn delete_rows_targets_nearest_survivors_at_bottom_edge() {
        let selection = range(MAX_ROWS - 2, 5, MAX_ROWS - 1, 8);
        let plan = plan_structural_selection(&selection, RibbonCommand::DeleteRows).unwrap();

        assert_eq!(plan.axis, StructuralAxis::Rows);
        assert_eq!(plan.at, MAX_ROWS - 2);
        assert_eq!(plan.count, 2);
        assert_eq!(plan.target_range, range(MAX_ROWS - 4, 5, MAX_ROWS - 3, 8));
    }

    #[test]
    fn insert_columns_uses_selected_width_and_preserves_selected_rows() {
        let selection = range(9, 7, 5, 4);
        let plan = plan_structural_selection(&selection, RibbonCommand::InsertColumns).unwrap();

        assert_eq!(plan.axis, StructuralAxis::Columns);
        assert_eq!(plan.at, 4);
        assert_eq!(plan.count, 4);
        assert_eq!(plan.target_range, range(5, 4, 9, 7));
    }

    #[test]
    fn delete_columns_targets_nearest_survivors_at_right_edge() {
        let selection = range(2, MAX_COLUMNS - 2, 4, MAX_COLUMNS - 1);
        let plan = plan_structural_selection(&selection, RibbonCommand::DeleteColumns).unwrap();

        assert_eq!(plan.axis, StructuralAxis::Columns);
        assert_eq!(plan.at, MAX_COLUMNS - 2);
        assert_eq!(plan.count, 2);
        assert_eq!(
            plan.target_range,
            range(2, MAX_COLUMNS - 4, 4, MAX_COLUMNS - 3)
        );
    }

    #[test]
    fn delete_preserves_selected_band_size_when_survivors_are_fewer() {
        let selection = range(0, 4, MAX_ROWS - 2, 6);
        let plan = plan_structural_selection(&selection, RibbonCommand::DeleteRows).unwrap();

        assert_eq!(plan.count, MAX_ROWS - 1);
        assert_eq!(plan.target_range, range(0, 4, MAX_ROWS - 2, 6));
    }

    #[test]
    fn rejects_non_structural_malformed_out_of_bounds_and_overflow_inputs() {
        let selection = range(1, 2, 3, 4);
        assert_eq!(
            plan_structural_selection(&selection, RibbonCommand::ClearRange),
            Err(StructuralSelectionError::UnsupportedCommand)
        );

        let mut malformed = selection.clone();
        malformed.end.sheet = SheetId(8);
        assert_eq!(
            plan_structural_selection(&malformed, RibbonCommand::InsertRows),
            Err(StructuralSelectionError::InvalidSelection)
        );

        let out_of_bounds = range(MAX_ROWS, 0, MAX_ROWS, 0);
        assert_eq!(
            plan_structural_selection(&out_of_bounds, RibbonCommand::InsertRows),
            Err(StructuralSelectionError::OutOfBounds)
        );

        let overflow = range(0, 0, u32::MAX, 0);
        assert_eq!(
            plan_structural_selection(&overflow, RibbonCommand::InsertRows),
            Err(StructuralSelectionError::Overflow)
        );
    }

    #[test]
    fn rejects_deleting_the_entire_structural_axis_without_a_survivor() {
        let all_rows = range(0, 3, MAX_ROWS - 1, 3);
        assert_eq!(
            plan_structural_selection(&all_rows, RibbonCommand::DeleteRows),
            Err(StructuralSelectionError::NoSurvivingTarget)
        );
    }
}
