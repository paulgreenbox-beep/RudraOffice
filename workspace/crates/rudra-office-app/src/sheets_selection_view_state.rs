//! Per-sheet snapshots for restoring the canonical spreadsheet selection and view.
//!
//! This stores a copy of the current selection for later restoration; it is not
//! an independent active-selection model. The owning document session remains
//! responsible for applying a stored selection back to the spreadsheet UI.

use std::collections::BTreeMap;

use sheets_core::SheetId;
use sheets_ui::{SelectionKind, SelectionState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StoredSheetSelection {
    pub(crate) selection: SelectionState,
    pub(crate) scroll_x: u32,
    pub(crate) scroll_y: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SheetsSelectionViewStates {
    by_sheet: BTreeMap<SheetId, StoredSheetSelection>,
}

impl SheetsSelectionViewStates {
    /// Remember a restorable selection and its scroll position for its sheet.
    ///
    /// Empty, inactive, or cross-sheet selections are ignored because they do
    /// not identify a valid range to restore.
    pub(crate) fn remember(&mut self, selection: SelectionState, scroll_x: u32, scroll_y: u32) {
        if selection.kind == SelectionKind::None {
            return;
        }

        let Some(range) = selection.range() else {
            return;
        };

        self.by_sheet.insert(
            range.sheet,
            StoredSheetSelection {
                selection,
                scroll_x,
                scroll_y,
            },
        );
    }

    pub(crate) fn for_sheet(&self, sheet: SheetId) -> Option<&StoredSheetSelection> {
        self.by_sheet.get(&sheet)
    }

    #[cfg(test)]
    pub(crate) fn remove_sheet(&mut self, sheet: SheetId) {
        let _ = self.by_sheet.remove(&sheet);
    }

    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.by_sheet.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::SheetsSelectionViewStates;
    use sheets_core::{CellAddress, ColumnIndex, RowIndex, SheetId};
    use sheets_ui::{SelectionKind, SelectionState};

    fn cell(sheet: SheetId, row: u32, column: u32) -> CellAddress {
        CellAddress::new(sheet, RowIndex(row), ColumnIndex(column))
    }

    fn selection_at(sheet: SheetId, row: u32, column: u32) -> SelectionState {
        let address = cell(sheet, row, column);
        SelectionState {
            kind: SelectionKind::Cell,
            anchor: Some(address),
            focus: Some(address),
            ..SelectionState::default()
        }
    }

    #[test]
    fn selections_and_scroll_positions_are_isolated_per_sheet() {
        let first_sheet = SheetId(1);
        let second_sheet = SheetId(2);
        let first_selection = selection_at(first_sheet, 3, 4);
        let second_selection = selection_at(second_sheet, 7, 8);
        let mut states = SheetsSelectionViewStates::default();

        states.remember(first_selection.clone(), 11, 12);
        states.remember(second_selection.clone(), 21, 22);

        let first = states.for_sheet(first_sheet).unwrap();
        assert_eq!(first.selection, first_selection);
        assert_eq!((first.scroll_x, first.scroll_y), (11, 12));

        let second = states.for_sheet(second_sheet).unwrap();
        assert_eq!(second.selection, second_selection);
        assert_eq!((second.scroll_x, second.scroll_y), (21, 22));
    }

    #[test]
    fn invalid_or_anchorless_selections_are_not_stored() {
        let first_sheet = SheetId(1);
        let other_sheet = SheetId(2);
        let mut states = SheetsSelectionViewStates::default();

        states.remember(SelectionState::default(), 1, 2);
        states.remember(
            SelectionState {
                kind: SelectionKind::Cell,
                anchor: None,
                focus: Some(cell(first_sheet, 1, 1)),
                ..SelectionState::default()
            },
            3,
            4,
        );
        states.remember(
            SelectionState {
                kind: SelectionKind::Range,
                anchor: Some(cell(first_sheet, 1, 1)),
                focus: Some(cell(other_sheet, 2, 2)),
                ..SelectionState::default()
            },
            5,
            6,
        );
        states.remember(
            SelectionState {
                kind: SelectionKind::None,
                anchor: Some(cell(first_sheet, 2, 2)),
                focus: Some(cell(first_sheet, 2, 2)),
                ..SelectionState::default()
            },
            7,
            8,
        );

        assert!(states.for_sheet(first_sheet).is_none());
        assert!(states.for_sheet(other_sheet).is_none());
    }

    #[test]
    fn restoring_a_selection_clone_does_not_change_the_stored_selection() {
        let sheet = SheetId(3);
        let original = selection_at(sheet, 4, 5);
        let mut states = SheetsSelectionViewStates::default();
        states.remember(original.clone(), 30, 40);

        let mut restored = states.for_sheet(sheet).unwrap().selection.clone();
        restored.focus = Some(cell(sheet, 9, 10));

        assert_ne!(restored, original);
        assert_eq!(states.for_sheet(sheet).unwrap().selection, original);
    }

    #[test]
    fn canonical_selection_extent_is_preserved() {
        let sheet = SheetId(6);
        let mut selection = selection_at(sheet, 4, 5);
        selection.extent = selection.range();
        let expected = selection.clone();
        let mut states = SheetsSelectionViewStates::default();

        states.remember(selection, 13, 14);

        assert_eq!(states.for_sheet(sheet).unwrap().selection, expected);
    }

    #[test]
    fn removed_sheets_and_clear_purge_saved_state() {
        let removed_sheet = SheetId(4);
        let retained_sheet = SheetId(5);
        let mut states = SheetsSelectionViewStates::default();
        states.remember(selection_at(removed_sheet, 1, 1), 1, 1);
        states.remember(selection_at(retained_sheet, 2, 2), 2, 2);

        states.remove_sheet(removed_sheet);
        assert!(states.for_sheet(removed_sheet).is_none());
        assert!(states.for_sheet(retained_sheet).is_some());

        states.clear();
        assert!(states.for_sheet(retained_sheet).is_none());
    }
}
