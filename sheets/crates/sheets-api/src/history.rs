//! Bounded, document-owned reversible canonical mutations.
//!
//! Entries retain changed fields only. Calculated values and whole-workbook
//! snapshots are never retained. Structural edits use the same bounded deltas.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::mem::size_of;

use sheets_core::{
    AutoFilter, CellAddress, CellFormat, CellInput, CellRange, Change, ChangeSet, ColumnIndex,
    CoreError, FilterCriterion, NumberFormat, Revision, RowIndex, SheetId, Workbook,
    WorkbookSnapshot,
};

/// Combined limits for both history directions. Zero disables the corresponding
/// capacity. A single transaction exceeding a limit forms a history barrier.
/// Sort/filter and structural commands reject that transaction before mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryLimits {
    pub max_entries: usize,
    /// Two endpoint slots per changed cell; merge/axis metadata uses byte budget.
    pub max_retained_cells: usize,
    /// Conservative payload estimate, including owned string capacities.
    pub max_retained_bytes: usize,
}

impl Default for HistoryLimits {
    fn default() -> Self {
        Self {
            max_entries: 100,
            max_retained_cells: 100_000,
            max_retained_bytes: 16 * 1024 * 1024,
        }
    }
}

/// History capacity and cursor, not a saved/dirty-state indicator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryStats {
    pub undo_entries: usize,
    pub redo_entries: usize,
    pub retained_cells: usize,
    pub retained_bytes: usize,
    pub evicted_entries: usize,
    pub barriers: usize,
}

#[derive(Debug, Default)]
pub(crate) struct History {
    undo: VecDeque<HistoryEntry>,
    redo: VecDeque<HistoryEntry>,
    limits: HistoryLimits,
    evicted_entries: usize,
    barriers: usize,
}

impl History {
    pub(crate) fn limits(&self) -> HistoryLimits {
        self.limits
    }

    pub(crate) fn set_limits(&mut self, limits: HistoryLimits) {
        self.limits = limits;
        self.enforce_limits();
    }

    pub(crate) fn stats(&self) -> HistoryStats {
        let (retained_cells, retained_bytes) =
            self.undo
                .iter()
                .chain(&self.redo)
                .fold((0usize, 0usize), |(cells, bytes), entry| {
                    (
                        cells.saturating_add(entry.retained_cells()),
                        bytes.saturating_add(entry.retained_bytes()),
                    )
                });
        HistoryStats {
            undo_entries: self.undo.len(),
            redo_entries: self.redo.len(),
            retained_cells,
            retained_bytes,
            evicted_entries: self.evicted_entries,
            barriers: self.barriers,
        }
    }

    pub(crate) fn undo_entry(&self) -> Option<&HistoryEntry> {
        self.undo.back()
    }

    pub(crate) fn redo_entry(&self) -> Option<&HistoryEntry> {
        self.redo.back()
    }

    pub(crate) fn position(&self, undo: bool, sheet: Option<SheetId>) -> Option<usize> {
        let entries = if undo { &self.undo } else { &self.redo };
        match sheet {
            Some(sheet) => entries.iter().rposition(|entry| entry.targets_sheet(sheet)),
            None => entries.len().checked_sub(1),
        }
    }

    /// Returns the sheet whose current state or history order prevents replay.
    /// No mutation or engine evaluation may occur before this check.
    pub(crate) fn replay_conflict(
        &self,
        undo: bool,
        position: usize,
        workbook: &Workbook,
    ) -> Option<SheetId> {
        let entry = self.entry_at(undo, position)?;
        if let Some(sheet) = entry.endpoint_conflict(undo, workbook) {
            return Some(sheet);
        }

        // Active later structural edits can move coordinates retained by an
        // earlier entry. An active later edit on the structural owner, or a
        // foreign formula edit, must be replayed in its original order.
        for later in self
            .undo
            .iter()
            .filter(|later| later.origin_revision > entry.origin_revision)
        {
            if let Some(owner) = entry.structural_owner {
                if later.sheets.contains(&owner) {
                    return Some(owner);
                }
                if let Some(sheet) = later.foreign_formula_sheet(owner) {
                    return Some(sheet);
                }
                if let Some(later_owner) = later.structural_owner
                    && entry.sheets.contains(&later_owner)
                {
                    return Some(later_owner);
                }
            } else if let Some(owner) = later.structural_owner
                && (entry.depends_on_structural(later)
                    || entry.foreign_formula_sheet(owner).is_some())
            {
                // A formula may remain only in a retained input endpoint after
                // the live cell was overwritten. Current-cell structural deltas
                // do not cover that endpoint. Preserve the conservative
                // foreign-formula ordering already used for redo.
                return Some(owner);
            }
        }

        if !undo {
            // A later action cannot be redone before a structural edit it
            // originally followed. This also covers per-sheet redo ordering.
            for earlier in self
                .redo
                .iter()
                .filter(|earlier| earlier.origin_revision < entry.origin_revision)
            {
                if let Some(owner) = earlier.structural_owner
                    && (entry.depends_on_structural(earlier)
                        || entry.foreign_formula_sheet(owner).is_some())
                {
                    return Some(owner);
                }
            }
            if let Some(owner) = entry.structural_owner {
                for earlier in self
                    .redo
                    .iter()
                    .filter(|earlier| earlier.origin_revision < entry.origin_revision)
                {
                    if earlier.sheets.contains(&owner) {
                        return Some(owner);
                    }
                }
            }
        }
        None
    }

    pub(crate) fn entry_at(&self, undo: bool, position: usize) -> Option<&HistoryEntry> {
        if undo {
            self.undo.get(position)
        } else {
            self.redo.get(position)
        }
    }

    pub(crate) fn did_undo(&mut self, position: usize) {
        if let Some(entry) = self.undo.remove(position) {
            self.redo.push_back(entry);
        }
    }

    pub(crate) fn did_redo(&mut self, position: usize) {
        if let Some(entry) = self.redo.remove(position) {
            self.undo.push_back(entry);
        }
    }

    pub(crate) fn can_record_entry(&self, entry: &HistoryEntry) -> bool {
        self.limits.max_entries > 0
            && entry.retained_cells() <= self.limits.max_retained_cells
            && entry.retained_bytes() <= self.limits.max_retained_bytes
    }

    pub(crate) fn record(&mut self, entry: HistoryEntry) {
        self.redo.clear();
        if !self.can_record_entry(&entry) {
            self.barrier();
            return;
        }
        self.undo.push_back(entry);
        self.enforce_limits();
    }

    pub(crate) fn barrier(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.barriers = self.barriers.saturating_add(1);
    }

    pub(crate) fn checkpoint(&mut self) {
        *self = Self {
            limits: self.limits,
            ..Self::default()
        };
    }

    fn enforce_limits(&mut self) {
        loop {
            let stats = self.stats();
            if stats.undo_entries.saturating_add(stats.redo_entries) <= self.limits.max_entries
                && stats.retained_cells <= self.limits.max_retained_cells
                && stats.retained_bytes <= self.limits.max_retained_bytes
            {
                return;
            }
            // Preserve the nearest transitions in each direction: oldest past
            // first, then furthest future. Never remove an entry in the middle.
            if self
                .undo
                .pop_front()
                .or_else(|| self.redo.pop_front())
                .is_none()
            {
                return;
            }
            self.evicted_entries = self.evicted_entries.saturating_add(1);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Endpoints<T> {
    before: T,
    after: T,
}

impl<T> Endpoints<T> {
    fn target(&self, undo: bool) -> &T {
        if undo { &self.before } else { &self.after }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct CellState {
    input: Option<CellInput>,
    format: Option<CellFormat>,
}

/// Checks the clipboard's single-entry history budget before `Workbook::prepare`
/// clones the canonical workbook. Only borrowed cell inputs and formats are
/// compared; no existing endpoint strings or styles are cloned here.
pub(crate) fn preflight_clipboard_history_budget(
    workbook: &Workbook,
    changes: &ChangeSet,
    limits: HistoryLimits,
) -> Result<(), crate::ApiTransactionError> {
    #[derive(Clone, Copy, PartialEq)]
    struct BorrowedCellState<'a> {
        input: Option<&'a CellInput>,
        format: Option<&'a CellFormat>,
    }

    static EMPTY_INPUT: CellInput = CellInput::Empty;

    let mut states: BTreeMap<CellAddress, Endpoints<BorrowedCellState<'_>>> = BTreeMap::new();
    let mut merges: BTreeMap<&CellRange, Endpoints<bool>> = BTreeMap::new();
    for change in &changes.changes {
        match change {
            Change::SetCellInput { address, .. }
            | Change::SetCellFormat { address, .. }
            | Change::ClearCell { address, .. }
            | Change::ClearCellFormat { address, .. } => {
                let state = states.entry(*address).or_insert_with(|| {
                    let current = BorrowedCellState {
                        input: workbook.cell_input(address.sheet, *address),
                        format: workbook.cell_format(address.sheet, *address),
                    };
                    Endpoints {
                        before: current,
                        after: current,
                    }
                });
                match change {
                    Change::SetCellInput { input, .. } => {
                        state.after.input = Some(input);
                        if matches!(input, CellInput::Empty) && state.after.format.is_none() {
                            state.after.input = None;
                        }
                    }
                    Change::ClearCell { .. } => {
                        state.after.input = state.after.format.map(|_| &EMPTY_INPUT);
                    }
                    Change::SetCellFormat { format, .. } => state.after.format = Some(format),
                    Change::ClearCellFormat { .. } => state.after.format = None,
                    _ => unreachable!(),
                }
            }
            Change::MergeCells { range, .. } | Change::UnmergeCells { range, .. } => {
                let state = merges.entry(range).or_insert_with(|| {
                    let present =
                        workbook.merge_at(range.sheet, range.start).as_ref() == Some(range);
                    Endpoints {
                        before: present,
                        after: present,
                    }
                });
                state.after = matches!(change, Change::MergeCells { .. });
            }
            _ => {
                // The caller uses this preflight only for clipboard cell and
                // merge commands; other mutations have their own budget path.
                return Err(crate::ApiTransactionError::DocumentLimitExceeded(
                    "clipboard history preflight received a non-clipboard change".into(),
                ));
            }
        }
    }

    let mut cell_count = 0usize;
    let mut merge_count = 0usize;
    let mut sheets = BTreeSet::new();
    let mut endpoint_bytes = 0usize;
    for (address, state) in states {
        let input_changed = state.before.input != state.after.input;
        let format_changed = state.before.format != state.after.format;
        if !input_changed && !format_changed {
            continue;
        }
        cell_count = cell_count.saturating_add(1);
        sheets.insert(address.sheet);
        if input_changed {
            endpoint_bytes = endpoint_bytes
                .saturating_add(size_of::<Endpoints<Option<CellInput>>>())
                .saturating_add(
                    [state.before.input, state.after.input]
                        .into_iter()
                        .flatten()
                        .fold(0usize, |bytes, input| {
                            bytes.saturating_add(match input {
                                CellInput::Text(text) | CellInput::Formula(text) => text.len(),
                                _ => 0,
                            })
                        }),
                );
        }
        if format_changed {
            endpoint_bytes =
                endpoint_bytes
                    .saturating_add(size_of::<Endpoints<Option<CellFormat>>>())
                    .saturating_add(
                        [state.before.format, state.after.format]
                            .into_iter()
                            .flatten()
                            .map(|format| {
                                format.font.family.len().saturating_add(
                                    match &format.number_format {
                                        NumberFormat::Custom(text)
                                        | NumberFormat::Currency { symbol: text, .. } => text.len(),
                                        _ => 0,
                                    },
                                )
                            })
                            .fold(0usize, |bytes, format_bytes| {
                                bytes.saturating_add(format_bytes)
                            }),
                    );
        }
    }
    for (range, presence) in merges {
        if presence.before != presence.after {
            merge_count = merge_count.saturating_add(1);
            sheets.insert(range.sheet);
        }
    }
    // No-op clipboard commands must remain allowed when history is disabled.
    if cell_count == 0 && merge_count == 0 {
        return Ok(());
    }

    // Vec::push uses geometric growth. This upper bound also covers the small
    // initial capacities in the actual HistoryEntry vectors.
    let vector_capacity = |len: usize| {
        if len == 0 {
            0
        } else {
            len.checked_next_power_of_two().unwrap_or(usize::MAX).max(4)
        }
    };
    let required_cell_endpoints = cell_count.saturating_mul(2);
    let required_bytes = size_of::<HistoryEntry>()
        .saturating_add(vector_capacity(cell_count).saturating_mul(size_of::<CellDelta>()))
        .saturating_add(vector_capacity(merge_count).saturating_mul(size_of::<MergeDelta>()))
        .saturating_add(vector_capacity(sheets.len()).saturating_mul(size_of::<SheetId>()))
        .saturating_add(endpoint_bytes);
    if limits.max_entries == 0
        || required_cell_endpoints > limits.max_retained_cells
        || required_bytes > limits.max_retained_bytes
    {
        return Err(crate::ApiTransactionError::HistoryBudgetExceeded {
            required_cell_endpoints,
            required_bytes,
            limits,
        });
    }
    Ok(())
}

impl CellState {
    fn read(workbook: &Workbook, address: CellAddress) -> Self {
        Self {
            input: workbook.cell_input(address.sheet, address).cloned(),
            format: workbook.cell_format(address.sheet, address).cloned(),
        }
    }
}

fn prepared_cell_states(
    workbook: &Workbook,
    changes: &ChangeSet,
) -> Result<BTreeMap<CellAddress, Endpoints<CellState>>, CoreError> {
    let mut states: BTreeMap<CellAddress, Endpoints<CellState>> = BTreeMap::new();
    for change in &changes.changes {
        let address = match change {
            Change::SetCellInput { address, .. }
            | Change::ClearCell { address, .. }
            | Change::SetCellFormat { address, .. }
            | Change::ClearCellFormat { address, .. } => *address,
            _ => continue,
        };
        let states = states.entry(address).or_insert_with(|| {
            let state = CellState::read(workbook, address);
            Endpoints {
                before: state.clone(),
                after: state,
            }
        });
        let state = &mut states.after;
        match change {
            Change::SetCellInput { input, .. } => {
                state.input = Some(match input {
                    CellInput::Formula(formula) => CellInput::formula(formula.clone())?,
                    other => other.clone(),
                });
                if state.input == Some(CellInput::Empty) && state.format.is_none() {
                    state.input = None;
                }
            }
            Change::ClearCell { .. } => {
                state.input = state.format.as_ref().map(|_| CellInput::Empty);
            }
            Change::SetCellFormat { format, .. } => state.format = Some(format.clone()),
            Change::ClearCellFormat { .. } => state.format = None,
            _ => unreachable!(),
        }
    }
    Ok(states)
}

#[derive(Debug)]
struct CellDelta {
    address: CellAddress,
    input: Option<Box<Endpoints<Option<CellInput>>>>,
    format: Option<Box<Endpoints<Option<CellFormat>>>>,
}

#[derive(Debug)]
struct MergeDelta {
    range: sheets_core::CellRange,
    presence: Endpoints<bool>,
}

#[derive(Debug)]
struct AxisDelta<I> {
    sheet: SheetId,
    index: I,
    value: Endpoints<Option<f64>>,
}

#[derive(Debug, Default)]
pub(crate) struct HistoryEntry {
    cells: Vec<CellDelta>,
    merges: Vec<MergeDelta>,
    rows: Vec<AxisDelta<RowIndex>>,
    columns: Vec<AxisDelta<ColumnIndex>>,
    filters: Vec<SheetDelta<Option<AutoFilter>>>,
    manual_hidden: Vec<SheetDelta<Vec<RowIndex>>>,
    manual_hidden_columns: Vec<SheetDelta<Vec<ColumnIndex>>>,
    sheets: Vec<SheetId>,
    structural_owner: Option<SheetId>,
    clipboard_cut_owner: Option<SheetId>,
    origin_revision: Revision,
}

#[derive(Debug)]
struct SheetDelta<T> {
    sheet: SheetId,
    value: Endpoints<T>,
}

pub(crate) enum Recording {
    NoMutation,
    Entry(Box<HistoryEntry>),
    Barrier,
}

pub(crate) fn same_model(before: &WorkbookSnapshot, after: &WorkbookSnapshot) -> bool {
    before.sheets.len() == after.sheets.len()
        && before.sheets.iter().zip(&after.sheets).all(|(a, b)| {
            a.id == b.id
                && a.name == b.name
                && a.merges == b.merges
                && a.row_dimensions == b.row_dimensions
                && a.column_dimensions == b.column_dimensions
                && a.auto_filter == b.auto_filter
                && a.manual_hidden_rows == b.manual_hidden_rows
                && a.manual_hidden_columns == b.manual_hidden_columns
                && a.cells
                    .iter()
                    .filter(|c| c.input != CellInput::Empty || c.format.is_some())
                    .eq(b
                        .cells
                        .iter()
                        .filter(|c| c.input != CellInput::Empty || c.format.is_some()))
        })
}

impl HistoryEntry {
    pub(crate) fn mark_structural(&mut self, sheet: SheetId) {
        self.structural_owner = Some(sheet);
    }

    pub(crate) fn mark_clipboard_cut_owner(&mut self, destination: SheetId) {
        self.clipboard_cut_owner = Some(destination);
    }

    fn targets_sheet(&self, sheet: SheetId) -> bool {
        if let Some(owner) = self.clipboard_cut_owner {
            return owner == sheet;
        }
        match self.structural_owner {
            Some(owner) => owner == sheet,
            None => self.sheets.contains(&sheet),
        }
    }

    pub(crate) fn single_sheet(&self) -> Option<SheetId> {
        if let Some(owner) = self.clipboard_cut_owner {
            return Some(owner);
        }
        if let Some(owner) = self.structural_owner {
            return Some(owner);
        }
        match self.sheets.as_slice() {
            [sheet] => Some(*sheet),
            _ => None,
        }
    }

    fn endpoint_conflict(&self, undo: bool, workbook: &Workbook) -> Option<SheetId> {
        for &sheet in &self.sheets {
            if workbook.sheet_name(sheet).is_none() {
                return Some(sheet);
            }
        }
        for delta in &self.cells {
            let sheet = delta.address.sheet;
            if delta.input.as_ref().is_some_and(|input| {
                workbook.cell_input(sheet, delta.address) != input.target(!undo).as_ref()
            }) || delta.format.as_ref().is_some_and(|format| {
                workbook.cell_format(sheet, delta.address) != format.target(!undo).as_ref()
            }) {
                return Some(sheet);
            }
        }
        for delta in &self.merges {
            let actual = workbook.merge_at(delta.range.sheet, delta.range.start);
            let exact_range_present = actual.as_ref() == Some(&delta.range);
            if exact_range_present != *delta.presence.target(!undo) {
                return Some(delta.range.sheet);
            }
        }
        for delta in &self.rows {
            if workbook.row_height(delta.sheet, delta.index) != *delta.value.target(!undo) {
                return Some(delta.sheet);
            }
        }
        for delta in &self.columns {
            if workbook.column_width(delta.sheet, delta.index) != *delta.value.target(!undo) {
                return Some(delta.sheet);
            }
        }
        for delta in &self.filters {
            if workbook.auto_filter(delta.sheet) != delta.value.target(!undo).as_ref() {
                return Some(delta.sheet);
            }
        }
        for delta in &self.manual_hidden {
            if !workbook
                .manual_hidden_rows(delta.sheet)
                .is_some_and(|rows| rows.iter().eq(delta.value.target(!undo).iter()))
            {
                return Some(delta.sheet);
            }
        }
        for delta in &self.manual_hidden_columns {
            if !workbook
                .manual_hidden_columns(delta.sheet)
                .is_some_and(|columns| columns.iter().eq(delta.value.target(!undo).iter()))
            {
                return Some(delta.sheet);
            }
        }
        None
    }

    fn foreign_formula_sheet(&self, owner: SheetId) -> Option<SheetId> {
        self.cells.iter().find_map(|delta| {
            if delta.address.sheet == owner {
                return None;
            }
            let input = delta.input.as_ref()?;
            (matches!(input.before.as_ref(), Some(CellInput::Formula(_)))
                || matches!(input.after.as_ref(), Some(CellInput::Formula(_))))
            .then_some(delta.address.sheet)
        })
    }

    fn depends_on_structural(&self, structural: &Self) -> bool {
        let Some(owner) = structural.structural_owner else {
            return false;
        };
        self.sheets.contains(&owner)
            || self.cells.iter().any(|cell| {
                cell.input.is_some()
                    && structural
                        .cells
                        .iter()
                        .any(|changed| changed.input.is_some() && changed.address == cell.address)
            })
    }

    pub(crate) fn prepare(
        workbook: &Workbook,
        changes: &ChangeSet,
        before: &WorkbookSnapshot,
        after: &WorkbookSnapshot,
        preserve_materialized_empty: bool,
    ) -> Result<Recording, CoreError> {
        let exact_states = if preserve_materialized_empty {
            Some(prepared_cell_states(workbook, changes)?)
        } else {
            None
        };
        let unchanged = if preserve_materialized_empty {
            before.sheets == after.sheets
                && exact_states
                    .as_ref()
                    .is_some_and(|states| states.values().all(|state| state.before == state.after))
        } else {
            same_model(before, after)
        };
        if unchanged {
            return Ok(Recording::NoMutation);
        }
        if changes.changes.iter().any(|change| {
            matches!(
                change,
                Change::AddSheet { .. }
                    | Change::DeleteSheet { .. }
                    | Change::RenameSheet { .. }
                    | Change::MoveSheet { .. }
                    | Change::DuplicateSheet { .. }
                    | Change::InsertRows { .. }
                    | Change::DeleteRows { .. }
                    | Change::InsertColumns { .. }
                    | Change::DeleteColumns { .. }
            )
        }) {
            return Ok(Recording::Barrier);
        }

        let mut entry = Self {
            origin_revision: before.revision,
            ..Self::default()
        };
        let states = match exact_states {
            Some(states) => states,
            None => prepared_cell_states(workbook, changes)?,
        };
        for (address, state) in states {
            let input = (state.before.input != state.after.input).then(|| {
                Box::new(Endpoints {
                    before: state.before.input,
                    after: state.after.input,
                })
            });
            let format = (state.before.format != state.after.format).then(|| {
                Box::new(Endpoints {
                    before: state.before.format,
                    after: state.after.format,
                })
            });
            if input.is_some() || format.is_some() {
                entry.cells.push(CellDelta {
                    address,
                    input,
                    format,
                });
            }
        }
        for (a, b) in before.sheets.iter().zip(&after.sheets) {
            if a.auto_filter != b.auto_filter {
                entry.filters.push(SheetDelta {
                    sheet: a.id,
                    value: Endpoints {
                        before: a.auto_filter.clone(),
                        after: b.auto_filter.clone(),
                    },
                });
            }
            if a.manual_hidden_rows != b.manual_hidden_rows {
                entry.manual_hidden.push(SheetDelta {
                    sheet: a.id,
                    value: Endpoints {
                        before: a.manual_hidden_rows.clone(),
                        after: b.manual_hidden_rows.clone(),
                    },
                });
            }
            if a.manual_hidden_columns != b.manual_hidden_columns {
                entry.manual_hidden_columns.push(SheetDelta {
                    sheet: a.id,
                    value: Endpoints {
                        before: a.manual_hidden_columns.clone(),
                        after: b.manual_hidden_columns.clone(),
                    },
                });
            }
            if a.merges != b.merges {
                // Snapshot import permits overlaps; core MergeCells cannot
                // restore them. Do not retain an inverse that cannot execute.
                for merges in [&a.merges, &b.merges] {
                    if merges.iter().enumerate().any(|(index, range)| {
                        merges[index + 1..]
                            .iter()
                            .any(|other| range.intersects(other))
                    }) {
                        return Ok(Recording::Barrier);
                    }
                }
                for range in a.merges.iter().filter(|range| !b.merges.contains(range)) {
                    entry.merges.push(MergeDelta {
                        range: range.clone(),
                        presence: Endpoints {
                            before: true,
                            after: false,
                        },
                    });
                }
                for range in b.merges.iter().filter(|range| !a.merges.contains(range)) {
                    entry.merges.push(MergeDelta {
                        range: range.clone(),
                        presence: Endpoints {
                            before: false,
                            after: true,
                        },
                    });
                }
            }
            let old: BTreeMap<_, _> = a.row_dimensions.iter().map(|d| (d.row, d.height)).collect();
            let new: BTreeMap<_, _> = b.row_dimensions.iter().map(|d| (d.row, d.height)).collect();
            for index in old
                .keys()
                .chain(new.keys())
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
            {
                if old.get(&index) != new.get(&index) {
                    entry.rows.push(AxisDelta {
                        sheet: a.id,
                        index,
                        value: Endpoints {
                            before: old.get(&index).copied(),
                            after: new.get(&index).copied(),
                        },
                    });
                }
            }
            let old: BTreeMap<_, _> = a
                .column_dimensions
                .iter()
                .map(|d| (d.column, d.width))
                .collect();
            let new: BTreeMap<_, _> = b
                .column_dimensions
                .iter()
                .map(|d| (d.column, d.width))
                .collect();
            for index in old
                .keys()
                .chain(new.keys())
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
            {
                if old.get(&index) != new.get(&index) {
                    entry.columns.push(AxisDelta {
                        sheet: a.id,
                        index,
                        value: Endpoints {
                            before: old.get(&index).copied(),
                            after: new.get(&index).copied(),
                        },
                    });
                }
            }
        }
        entry.sheets = entry
            .cells
            .iter()
            .map(|delta| delta.address.sheet)
            .chain(entry.merges.iter().map(|delta| delta.range.sheet))
            .chain(entry.rows.iter().map(|delta| delta.sheet))
            .chain(entry.columns.iter().map(|delta| delta.sheet))
            .chain(entry.filters.iter().map(|delta| delta.sheet))
            .chain(entry.manual_hidden.iter().map(|delta| delta.sheet))
            .chain(entry.manual_hidden_columns.iter().map(|delta| delta.sheet))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(Recording::Entry(Box::new(entry)))
    }

    pub(crate) fn changes(&self, workbook: &Workbook, undo: bool) -> ChangeSet {
        let mut changes = ChangeSet::new();
        for delta in &self.filters {
            changes.push(Change::SetAutoFilter {
                sheet: delta.sheet,
                filter: delta.value.target(undo).clone(),
            });
        }
        for delta in &self.manual_hidden {
            changes.push(Change::SetManualHiddenRows {
                sheet: delta.sheet,
                rows: delta.value.target(undo).clone(),
            });
        }
        for delta in &self.manual_hidden_columns {
            changes.push(Change::SetManualHiddenColumns {
                sheet: delta.sheet,
                columns: delta.value.target(undo).clone(),
            });
        }
        // Remove changed topology before adding its replacement (overlap-safe).
        for delta in self.merges.iter().filter(|d| !*d.presence.target(undo)) {
            changes.push(Change::UnmergeCells {
                sheet: delta.range.sheet,
                range: delta.range.clone(),
            });
        }
        for delta in &self.cells {
            let mut target = CellState::read(workbook, delta.address);
            if let Some(input) = &delta.input {
                target.input = input.target(undo).clone();
            }
            if let Some(format) = &delta.format {
                target.format = format.target(undo).clone();
            }
            let sheet = delta.address.sheet;
            let address = delta.address;
            changes.push(Change::ClearCellFormat { sheet, address });
            changes.push(Change::SetCellInput {
                sheet,
                address,
                input: CellInput::Empty,
            });
            match target.input {
                Some(CellInput::Empty) => {
                    changes.push(Change::SetCellFormat {
                        sheet,
                        address,
                        format: target.format.clone().unwrap_or_default(),
                    });
                    changes.push(Change::SetCellInput {
                        sheet,
                        address,
                        input: CellInput::Empty,
                    });
                    if target.format.is_none() {
                        changes.push(Change::ClearCellFormat { sheet, address });
                    }
                }
                input => {
                    if let Some(input) = input {
                        changes.push(Change::SetCellInput {
                            sheet,
                            address,
                            input,
                        });
                    }
                    if let Some(format) = target.format {
                        changes.push(Change::SetCellFormat {
                            sheet,
                            address,
                            format,
                        });
                    }
                }
            }
        }
        for delta in &self.rows {
            changes.push(match delta.value.target(undo) {
                Some(height) => Change::SetRowDimension {
                    sheet: delta.sheet,
                    row: delta.index,
                    height: *height,
                },
                None => Change::ClearRowDimension {
                    sheet: delta.sheet,
                    row: delta.index,
                },
            });
        }
        for delta in &self.columns {
            changes.push(match delta.value.target(undo) {
                Some(width) => Change::SetColumnDimension {
                    sheet: delta.sheet,
                    column: delta.index,
                    width: *width,
                },
                None => Change::ClearColumnDimension {
                    sheet: delta.sheet,
                    column: delta.index,
                },
            });
        }
        for delta in self.merges.iter().filter(|d| *d.presence.target(undo)) {
            changes.push(Change::MergeCells {
                sheet: delta.range.sheet,
                range: delta.range.clone(),
            });
        }
        changes
    }

    pub(crate) fn retained_cells(&self) -> usize {
        self.cells.len().saturating_mul(2)
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        let mut bytes = size_of::<Self>()
            .saturating_add(
                self.filters
                    .capacity()
                    .saturating_mul(size_of::<SheetDelta<Option<AutoFilter>>>()),
            )
            .saturating_add(
                self.manual_hidden
                    .capacity()
                    .saturating_mul(size_of::<SheetDelta<Vec<RowIndex>>>()),
            )
            .saturating_add(
                self.manual_hidden_columns
                    .capacity()
                    .saturating_mul(size_of::<SheetDelta<Vec<ColumnIndex>>>()),
            )
            .saturating_add(self.sheets.capacity().saturating_mul(size_of::<SheetId>()))
            .saturating_add(self.cells.capacity().saturating_mul(size_of::<CellDelta>()))
            .saturating_add(
                self.merges
                    .capacity()
                    .saturating_mul(size_of::<MergeDelta>()),
            )
            .saturating_add(
                self.rows
                    .capacity()
                    .saturating_mul(size_of::<AxisDelta<RowIndex>>()),
            )
            .saturating_add(
                self.columns
                    .capacity()
                    .saturating_mul(size_of::<AxisDelta<ColumnIndex>>()),
            );
        for cell in &self.cells {
            if let Some(input) = &cell.input {
                bytes = bytes.saturating_add(size_of::<Endpoints<Option<CellInput>>>());
                for input in [&input.before, &input.after].into_iter().flatten() {
                    if let CellInput::Text(text) | CellInput::Formula(text) = input {
                        bytes = bytes.saturating_add(text.capacity());
                    }
                }
            }
            if let Some(format) = &cell.format {
                bytes = bytes.saturating_add(size_of::<Endpoints<Option<CellFormat>>>());
                for format in [&format.before, &format.after].into_iter().flatten() {
                    bytes = bytes.saturating_add(format.font.family.capacity());
                    if let NumberFormat::Custom(text)
                    | NumberFormat::Currency { symbol: text, .. } = &format.number_format
                    {
                        bytes = bytes.saturating_add(text.capacity());
                    }
                }
            }
        }
        for delta in &self.manual_hidden {
            bytes = bytes
                .saturating_add(
                    delta
                        .value
                        .before
                        .capacity()
                        .saturating_mul(size_of::<RowIndex>()),
                )
                .saturating_add(
                    delta
                        .value
                        .after
                        .capacity()
                        .saturating_mul(size_of::<RowIndex>()),
                );
        }
        for delta in &self.manual_hidden_columns {
            bytes = bytes
                .saturating_add(
                    delta
                        .value
                        .before
                        .capacity()
                        .saturating_mul(size_of::<ColumnIndex>()),
                )
                .saturating_add(
                    delta
                        .value
                        .after
                        .capacity()
                        .saturating_mul(size_of::<ColumnIndex>()),
                );
        }
        for delta in &self.filters {
            for filter in [&delta.value.before, &delta.value.after]
                .into_iter()
                .flatten()
            {
                // BTreeMap node allocation is estimated conservatively per key.
                bytes =
                    bytes.saturating_add(filter.criteria.len().saturating_mul(
                        size_of::<ColumnIndex>() + size_of::<FilterCriterion>() + 128,
                    ));
                for criterion in filter.criteria.values() {
                    match criterion {
                        FilterCriterion::Values(values) => {
                            bytes = bytes.saturating_add(
                                values.capacity().saturating_mul(size_of::<String>()),
                            );
                            for value in values {
                                bytes = bytes.saturating_add(value.capacity());
                            }
                        }
                        FilterCriterion::Equals(value)
                        | FilterCriterion::NotEquals(value)
                        | FilterCriterion::Contains(value) => {
                            bytes = bytes.saturating_add(value.capacity())
                        }
                        _ => {}
                    }
                }
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FormatPatchBounds, SpreadsheetApi, SpreadsheetEngine};
    use sheets_core::{
        CalculatedCell, CellAddress, CellFormat, CellInput, CellRange, CellValue, CoreError,
        EngineRequest, EngineResponse, Font, RowIndex,
    };
    use std::cell::Cell as StdCell;
    use std::rc::Rc;

    #[derive(Clone, Debug, Default)]
    struct EngineProbe {
        calls: Rc<StdCell<usize>>,
        fail_next: Rc<StdCell<bool>>,
        bad_response_next: Rc<StdCell<bool>>,
    }

    #[derive(Clone, Debug)]
    struct HistoryTestEngine(EngineProbe);

    impl SpreadsheetEngine for HistoryTestEngine {
        type Error = CoreError;

        fn evaluate(&mut self, request: EngineRequest) -> Result<EngineResponse, Self::Error> {
            self.0.calls.set(self.0.calls.get().saturating_add(1));
            if self.0.fail_next.replace(false) {
                return Err(CoreError::Unsupported("planned engine failure".into()));
            }
            if self.0.bad_response_next.replace(false) {
                let sheet = request.workbook().sheets[0].id;
                return Ok(EngineResponse::new(
                    request.base_revision(),
                    vec![CalculatedCell {
                        address: address(sheet, 0, 0),
                        value: CellValue::Number(777.0),
                    }],
                ));
            }
            Ok(EngineResponse::new(request.base_revision(), Vec::new()))
        }
    }

    fn test_api() -> (SpreadsheetApi<HistoryTestEngine>, EngineProbe) {
        let probe = EngineProbe::default();
        (SpreadsheetApi::new(HistoryTestEngine(probe.clone())), probe)
    }

    fn address(sheet: SheetId, row: u32, column: u32) -> CellAddress {
        CellAddress::new(sheet, RowIndex(row), ColumnIndex(column))
    }

    fn range(sheet: SheetId, start: (u32, u32), end: (u32, u32)) -> CellRange {
        CellRange::new(
            address(sheet, start.0, start.1),
            address(sheet, end.0, end.1),
        )
        .unwrap()
    }

    fn bounds(max_cells: u64) -> FormatPatchBounds {
        FormatPatchBounds {
            max_cells,
            max_sheets: 1,
        }
    }

    fn limits(
        max_entries: usize,
        max_retained_cells: usize,
        max_retained_bytes: usize,
    ) -> HistoryLimits {
        HistoryLimits {
            max_entries,
            max_retained_cells,
            max_retained_bytes,
        }
    }

    fn font_format(bold: bool, italic: bool) -> CellFormat {
        CellFormat {
            font: Font {
                bold,
                italic,
                ..Font::default()
            },
            ..CellFormat::default()
        }
    }

    #[test]
    fn sparse_edits_and_bulk_clear_restore_inputs_and_blank_cell_presence() {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        let a1 = address(sheet, 0, 0);
        let b1 = address(sheet, 0, 1);
        let c1 = address(sheet, 0, 2);
        let d1 = address(sheet, 0, 3);
        let formula = CellInput::formula("=A1+1").unwrap();
        let style = font_format(true, false);

        api.set_cell_input(sheet, a1, CellInput::Number(4.0))
            .unwrap();
        api.set_cell_input(sheet, b1, formula.clone()).unwrap();
        api.set_cell_input(sheet, c1, CellInput::Text("sparse text".into()))
            .unwrap();
        api.set_cell_format(sheet, d1, style.clone(), bounds(1))
            .unwrap();

        let before_clear = api.history_stats();
        api.clear_range(sheet, range(sheet, (0, 0), (0, 2)), bounds(3))
            .unwrap();
        assert_eq!(
            api.history_stats().undo_entries,
            before_clear.undo_entries + 1
        );
        for cell in [a1, b1, c1] {
            assert_eq!(api.document().cell_input(sheet, cell), None);
        }
        assert_eq!(api.document().cell_format(sheet, d1), Some(&style));

        api.undo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, a1),
            Some(&CellInput::Number(4.0))
        );
        assert_eq!(api.document().cell_input(sheet, b1), Some(&formula));
        assert_eq!(
            api.document().cell_input(sheet, c1),
            Some(&CellInput::Text("sparse text".into()))
        );
        assert_eq!(api.document().cell_format(sheet, d1), Some(&style));
        api.redo().unwrap();
        assert_eq!(api.document().cell_input(sheet, a1), None);
        assert_eq!(api.document().cell_input(sheet, b1), None);
        assert_eq!(api.document().cell_input(sheet, c1), None);

        // Canonical blank input is a no-op; a real edit must undo back to the
        // original format-only cell with absent raw input.
        let blank_state = api.history_stats();
        api.set_cell_input(sheet, d1, CellInput::Empty).unwrap();
        assert_eq!(api.document().cell_input(sheet, d1), None);
        assert_eq!(api.history_stats(), blank_state);
        api.set_cell_input(sheet, d1, CellInput::Number(6.0))
            .unwrap();
        api.undo().unwrap();
        assert_eq!(api.document().cell_input(sheet, d1), None);
        assert_eq!(api.document().cell_format(sheet, d1), Some(&style));
        api.redo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, d1),
            Some(&CellInput::Number(6.0))
        );
        // Real clearing can leave Some(Empty). Removing and restoring its
        // format must preserve that raw state, including an unformatted blank.
        api.clear_cell(sheet, d1).unwrap();
        api.clear_cell_format(sheet, d1).unwrap();
        assert_eq!(
            api.document().cell_input(sheet, d1),
            Some(&CellInput::Empty)
        );
        assert_eq!(api.document().cell_format(sheet, d1), None);
        api.undo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, d1),
            Some(&CellInput::Empty)
        );
        assert_eq!(api.document().cell_format(sheet, d1), Some(&style));
        api.redo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, d1),
            Some(&CellInput::Empty)
        );
        assert_eq!(api.document().cell_format(sheet, d1), None);
    }

    #[test]
    fn format_and_dimension_changes_restore_exact_prior_values_in_one_transaction() {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        let a1 = address(sheet, 0, 0);
        let original_format = font_format(false, true);
        let changed_format = font_format(true, false);
        api.set_cell_input(sheet, a1, CellInput::Text("keep".into()))
            .unwrap();
        api.set_cell_format(sheet, a1, original_format.clone(), bounds(1))
            .unwrap();
        api.set_row_dimension(sheet, RowIndex(0), 22.0).unwrap();
        api.set_column_dimension(sheet, ColumnIndex(0), 88.0)
            .unwrap();
        let before = api.document().snapshot();
        let before_rows = before
            .sheets
            .iter()
            .find(|item| item.id == sheet)
            .unwrap()
            .row_dimensions
            .clone();
        let before_columns = before
            .sheets
            .iter()
            .find(|item| item.id == sheet)
            .unwrap()
            .column_dimensions
            .clone();
        let before_entries = api.history_stats().undo_entries;

        let mut changes = ChangeSet::new();
        changes.push(Change::SetCellFormat {
            sheet,
            address: a1,
            format: changed_format.clone(),
        });
        changes.push(Change::SetRowDimension {
            sheet,
            row: RowIndex(0),
            height: 40.0,
        });
        changes.push(Change::SetColumnDimension {
            sheet,
            column: ColumnIndex(0),
            width: 120.0,
        });
        api.execute(changes).unwrap();
        assert_eq!(api.history_stats().undo_entries, before_entries + 1);
        api.undo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, a1),
            Some(&CellInput::Text("keep".into()))
        );
        assert_eq!(
            api.document().cell_format(sheet, a1),
            Some(&original_format)
        );
        let restored = api.document().snapshot();
        let restored_sheet = restored
            .sheets
            .iter()
            .find(|item| item.id == sheet)
            .unwrap();
        assert_eq!(restored_sheet.row_dimensions, before_rows);
        assert_eq!(restored_sheet.column_dimensions, before_columns);

        api.redo().unwrap();
        assert_eq!(api.document().cell_format(sheet, a1), Some(&changed_format));
        let changed = api.document().snapshot();
        let changed_sheet = changed.sheets.iter().find(|item| item.id == sheet).unwrap();
        let changed_rows = changed_sheet.row_dimensions.clone();
        let changed_columns = changed_sheet.column_dimensions.clone();

        let mut clear = ChangeSet::new();
        clear.push(Change::ClearCellFormat { sheet, address: a1 });
        clear.push(Change::ClearRowDimension {
            sheet,
            row: RowIndex(0),
        });
        clear.push(Change::ClearColumnDimension {
            sheet,
            column: ColumnIndex(0),
        });
        api.execute(clear).unwrap();
        api.undo().unwrap();
        assert_eq!(api.document().cell_format(sheet, a1), Some(&changed_format));
        let restored = api.document().snapshot();
        let restored_sheet = restored
            .sheets
            .iter()
            .find(|item| item.id == sheet)
            .unwrap();
        assert_eq!(restored_sheet.row_dimensions, changed_rows);
        assert_eq!(restored_sheet.column_dimensions, changed_columns);
        api.redo().unwrap();
        assert_eq!(api.document().cell_format(sheet, a1), None);
        let cleared = api.document().snapshot();
        let cleared_sheet = cleared.sheets.iter().find(|item| item.id == sheet).unwrap();
        assert!(cleared_sheet.row_dimensions.is_empty());
        assert!(cleared_sheet.column_dimensions.is_empty());
    }

    #[test]
    fn merge_and_multi_unmerge_restore_exact_topology() {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        let first = range(sheet, (0, 0), (1, 1));
        let second = range(sheet, (0, 2), (1, 3));
        api.safe_merge_cells(sheet, first.clone()).unwrap();
        api.safe_merge_cells(sheet, second.clone()).unwrap();
        let before_unmerge = api.history_stats().undo_entries;

        api.unmerge_intersecting_selection(sheet, range(sheet, (0, 1), (1, 2)))
            .unwrap();
        assert!(api.document().snapshot().sheets[0].merges.is_empty());
        assert_eq!(api.history_stats().undo_entries, before_unmerge + 1);
        api.undo().unwrap();
        assert_eq!(
            api.document().snapshot().sheets[0].merges,
            vec![first.clone(), second.clone()]
        );
        api.redo().unwrap();
        assert!(api.document().snapshot().sheets[0].merges.is_empty());

        let third = range(sheet, (3, 3), (4, 4));
        api.safe_merge_cells(sheet, third.clone()).unwrap();
        api.undo().unwrap();
        assert!(api.document().snapshot().sheets[0].merges.is_empty());
        api.redo().unwrap();
        assert_eq!(api.document().snapshot().sheets[0].merges, vec![third]);
    }

    #[test]
    fn dependent_formula_values_recalculate_after_undo_and_redo() {
        let mut api = SpreadsheetApi::production();
        let sheet = api.document().active_sheet();
        let a1 = address(sheet, 0, 0);
        let b1 = address(sheet, 0, 1);
        api.set_cell_input(sheet, a1, CellInput::Number(4.0))
            .unwrap();
        api.set_cell_input(sheet, b1, CellInput::formula("=A1+1").unwrap())
            .unwrap();
        api.set_cell_input(sheet, a1, CellInput::Number(9.0))
            .unwrap();
        assert_eq!(
            api.document().cell_value(sheet, b1),
            Some(&CellValue::Number(10.0))
        );

        api.undo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, a1),
            Some(&CellInput::Number(4.0))
        );
        assert_eq!(
            api.document().cell_value(sheet, b1),
            Some(&CellValue::Number(5.0))
        );
        api.redo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, a1),
            Some(&CellInput::Number(9.0))
        );
        assert_eq!(
            api.document().cell_value(sheet, b1),
            Some(&CellValue::Number(10.0))
        );
    }

    #[test]
    fn noops_navigation_invalid_commands_and_engine_failures_preserve_redo() {
        let (mut api, probe) = test_api();
        let sheet = api.document().active_sheet();
        api.add_sheet(Some("Other".into())).unwrap();
        let other = api.document().active_sheet();
        api.set_active_sheet(sheet).unwrap();
        let a1 = address(sheet, 0, 0);
        let unsafe_range = range(sheet, (0, 4), (1, 5));
        api.set_cell_input(
            sheet,
            address(sheet, 1, 5),
            CellInput::Text("concealed".into()),
        )
        .unwrap();
        let merged = range(sheet, (0, 0), (1, 1));
        api.safe_merge_cells(sheet, merged.clone()).unwrap();
        api.set_cell_input(sheet, a1, CellInput::Number(1.0))
            .unwrap();
        api.set_cell_input(sheet, a1, CellInput::Number(2.0))
            .unwrap();
        api.undo().unwrap();
        assert!(api.can_redo());

        let stable_revision = api.document().revision();
        let stable_stats = api.history_stats();
        let stable_calls = probe.calls.get();
        api.set_cell_input(sheet, a1, CellInput::Number(1.0))
            .unwrap();
        api.clear_range(sheet, range(sheet, (4, 4), (4, 4)), bounds(1))
            .unwrap();
        api.safe_merge_cells(sheet, merged).unwrap();
        api.unmerge_intersecting_selection(sheet, range(sheet, (5, 5), (6, 6)))
            .unwrap();
        assert_eq!(api.document().revision(), stable_revision);
        assert_eq!(api.history_stats(), stable_stats);
        assert_eq!(probe.calls.get(), stable_calls);
        assert!(api.can_redo());

        // Active-sheet navigation evaluates, but is not an undoable mutation.
        api.set_active_sheet(other).unwrap();
        assert_eq!(
            api.document().revision(),
            stable_revision.checked_next().unwrap()
        );
        assert_eq!(probe.calls.get(), stable_calls + 1);
        assert_eq!(api.history_stats(), stable_stats);
        assert!(api.can_redo());

        let too_large = range(sheet, (0, 0), (100, 100));
        let before_invalid = api.document().snapshot();
        let before_invalid_stats = api.history_stats();
        let before_invalid_calls = probe.calls.get();
        assert!(api.clear_range(sheet, too_large, bounds(10_000)).is_err());
        assert!(api.safe_merge_cells(sheet, unsafe_range).is_err());
        assert_eq!(api.document().snapshot(), before_invalid);
        assert_eq!(api.history_stats(), before_invalid_stats);
        assert_eq!(probe.calls.get(), before_invalid_calls);
        assert!(api.can_redo());

        probe.fail_next.set(true);
        assert!(
            api.set_cell_input(sheet, address(sheet, 3, 3), CellInput::Text("fail".into()))
                .is_err()
        );
        assert_eq!(api.document().snapshot(), before_invalid);
        assert_eq!(api.history_stats(), before_invalid_stats);
        assert!(api.can_redo());
        api.redo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, a1),
            Some(&CellInput::Number(2.0))
        );
    }

    #[test]
    fn failed_or_invalid_replay_keeps_workbook_and_history_cursor_atomic() {
        let (mut api, probe) = test_api();
        let sheet = api.document().active_sheet();
        let a1 = address(sheet, 0, 0);
        api.set_cell_input(sheet, a1, CellInput::Number(1.0))
            .unwrap();
        api.set_cell_input(sheet, a1, CellInput::Number(2.0))
            .unwrap();

        let before_undo = api.document().snapshot();
        let undo_stats = api.history_stats();
        probe.fail_next.set(true);
        assert!(api.undo().is_err());
        assert_eq!(api.document().snapshot(), before_undo);
        assert_eq!(api.history_stats(), undo_stats);
        assert!(api.can_undo());

        probe.bad_response_next.set(true);
        assert!(api.undo().is_err());
        assert_eq!(api.document().snapshot(), before_undo);
        assert_eq!(api.history_stats(), undo_stats);
        assert!(api.can_undo());

        api.undo().unwrap();
        let before_redo = api.document().snapshot();
        let redo_stats = api.history_stats();
        probe.fail_next.set(true);
        assert!(api.redo().is_err());
        assert_eq!(api.document().snapshot(), before_redo);
        assert_eq!(api.history_stats(), redo_stats);
        assert!(api.can_redo());

        probe.bad_response_next.set(true);
        assert!(api.redo().is_err());
        assert_eq!(api.document().snapshot(), before_redo);
        assert_eq!(api.history_stats(), redo_stats);
        assert!(api.can_redo());
        api.redo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, a1),
            Some(&CellInput::Number(2.0))
        );
    }

    #[test]
    fn independent_history_stays_with_owned_api_when_sessions_are_moved() {
        let (api_a, _) = test_api();
        let (api_b, _) = test_api();
        let token_a = api_a.document().token();
        let token_b = api_b.document().token();
        let sheet_a = api_a.document().active_sheet();
        let sheet_b = api_b.document().active_sheet();
        let a1 = address(sheet_a, 0, 0);
        let b1 = address(sheet_b, 0, 0);
        let mut api_a = api_a;
        let mut api_b = api_b;
        api_a
            .set_cell_input(sheet_a, a1, CellInput::Text("A old".into()))
            .unwrap();
        api_a
            .set_cell_input(sheet_a, a1, CellInput::Text("A new".into()))
            .unwrap();
        api_b
            .set_cell_input(sheet_b, b1, CellInput::Number(10.0))
            .unwrap();
        api_b
            .set_cell_input(sheet_b, b1, CellInput::Number(20.0))
            .unwrap();
        let b_before_a_undo = api_b.document().snapshot();

        let mut slot_a = Some(api_a);
        let mut slot_b = Some(api_b);
        std::mem::swap(&mut slot_a, &mut slot_b);
        assert_eq!(slot_b.as_ref().unwrap().document().token(), token_a);
        assert_eq!(slot_a.as_ref().unwrap().document().token(), token_b);
        slot_b.as_mut().unwrap().undo().unwrap();
        assert_eq!(
            slot_b.as_ref().unwrap().document().cell_input(sheet_a, a1),
            Some(&CellInput::Text("A old".into()))
        );
        assert_eq!(
            slot_a.as_ref().unwrap().document().snapshot(),
            b_before_a_undo
        );

        std::mem::swap(&mut slot_a, &mut slot_b);
        let a_before_b_undo = slot_a.as_ref().unwrap().document().snapshot();
        slot_b.as_mut().unwrap().undo().unwrap();
        assert_eq!(
            slot_b.as_ref().unwrap().document().cell_input(sheet_b, b1),
            Some(&CellInput::Number(10.0))
        );
        assert_eq!(
            slot_a.as_ref().unwrap().document().snapshot(),
            a_before_b_undo
        );
        assert_eq!(slot_a.as_ref().unwrap().document().token(), token_a);
        assert_eq!(slot_b.as_ref().unwrap().document().token(), token_b);
    }

    #[test]
    fn command_count_and_cell_budgets_evict_oldest_whole_transactions() {
        let (mut by_count, _) = test_api();
        let sheet = by_count.document().active_sheet();
        by_count.set_history_limits(limits(2, 100, 1_000_000));
        for (column, value) in [(0, 1.0), (1, 2.0), (2, 3.0)] {
            by_count
                .set_cell_input(sheet, address(sheet, 0, column), CellInput::Number(value))
                .unwrap();
        }
        assert_eq!(by_count.history_stats().undo_entries, 2);
        assert_eq!(by_count.history_stats().evicted_entries, 1);
        by_count.undo().unwrap();
        by_count.undo().unwrap();
        assert!(!by_count.can_undo());
        assert_eq!(
            by_count.document().cell_input(sheet, address(sheet, 0, 0)),
            Some(&CellInput::Number(1.0))
        );

        let (mut by_cells, _) = test_api();
        let sheet = by_cells.document().active_sheet();
        by_cells.set_history_limits(limits(10, 4, 1_000_000));
        for (column, value) in [(0, 1.0), (1, 2.0), (2, 3.0)] {
            by_cells
                .set_cell_input(sheet, address(sheet, 0, column), CellInput::Number(value))
                .unwrap();
        }
        assert_eq!(by_cells.history_stats().retained_cells, 4);
        assert_eq!(by_cells.history_stats().undo_entries, 2);
        assert_eq!(by_cells.history_stats().evicted_entries, 1);
        by_cells.undo().unwrap();
        by_cells.undo().unwrap();
        assert_eq!(
            by_cells.document().cell_input(sheet, address(sheet, 0, 0)),
            Some(&CellInput::Number(1.0))
        );
    }

    fn retained_bytes_for_text(text: String) -> usize {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        api.set_cell_input(sheet, address(sheet, 0, 0), CellInput::Text(text))
            .unwrap();
        api.history_stats().retained_bytes
    }

    #[test]
    fn byte_budget_evicts_oldest_and_oversized_transaction_forms_barrier() {
        let small_bytes = retained_bytes_for_text("x".into());
        let large_text = "Ж🙂".repeat(2_048);
        let large_bytes = retained_bytes_for_text(large_text.clone());
        assert!(large_bytes > small_bytes);

        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        api.set_history_limits(limits(10, 100, large_bytes));
        api.set_cell_input(sheet, address(sheet, 0, 0), CellInput::Text("x".into()))
            .unwrap();
        api.set_cell_input(sheet, address(sheet, 0, 1), CellInput::Text(large_text))
            .unwrap();
        assert_eq!(api.history_stats().undo_entries, 1);
        assert_eq!(api.history_stats().evicted_entries, 1);
        assert_eq!(api.history_stats().retained_bytes, large_bytes);
        api.undo().unwrap();
        assert_eq!(api.document().cell_input(sheet, address(sheet, 0, 1)), None);
        assert_eq!(
            api.document().cell_input(sheet, address(sheet, 0, 0)),
            Some(&CellInput::Text("x".into()))
        );

        let (mut oversized, _) = test_api();
        let sheet = oversized.document().active_sheet();
        oversized.set_history_limits(limits(10, 1, 1_000_000));
        oversized
            .set_cell_input(sheet, address(sheet, 0, 0), CellInput::Number(5.0))
            .unwrap();
        assert_eq!(
            oversized.document().cell_input(sheet, address(sheet, 0, 0)),
            Some(&CellInput::Number(5.0))
        );
        assert!(!oversized.can_undo());
        assert_eq!(oversized.history_stats().barriers, 1);
    }

    #[test]
    fn tightening_limits_preserves_nearest_past_and_future_cursor() {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        for (column, value) in [(0, 1.0), (1, 2.0), (2, 3.0)] {
            api.set_cell_input(sheet, address(sheet, 0, column), CellInput::Number(value))
                .unwrap();
        }
        api.undo().unwrap();
        let old_limits = api.history_limits();
        api.set_history_limits(limits(
            2,
            old_limits.max_retained_cells,
            old_limits.max_retained_bytes,
        ));
        assert_eq!(api.history_stats().undo_entries, 1);
        assert_eq!(api.history_stats().redo_entries, 1);
        assert_eq!(api.history_stats().evicted_entries, 1);

        api.undo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, address(sheet, 0, 0)),
            Some(&CellInput::Number(1.0))
        );
        assert_eq!(api.document().cell_input(sheet, address(sheet, 0, 1)), None);
        assert_eq!(api.document().cell_input(sheet, address(sheet, 0, 2)), None);
        assert!(!api.can_undo());
        api.redo().unwrap();
        api.redo().unwrap();
        assert_eq!(
            api.document().cell_input(sheet, address(sheet, 0, 1)),
            Some(&CellInput::Number(2.0))
        );
        assert_eq!(
            api.document().cell_input(sheet, address(sheet, 0, 2)),
            Some(&CellInput::Number(3.0))
        );
    }

    #[test]
    fn xlsx_load_checkpoints_history_preserves_limits_and_failed_load_is_atomic() {
        let mut source = SpreadsheetApi::production();
        let source_sheet = source.document().active_sheet();
        source
            .set_cell_input(
                source_sheet,
                address(source_sheet, 0, 0),
                CellInput::Text("loaded".into()),
            )
            .unwrap();
        let package = source.export_xlsx().unwrap();

        let mut target = SpreadsheetApi::production();
        let limits = limits(7, 321, 65_432);
        target.set_history_limits(limits);
        let target_sheet = target.document().active_sheet();
        target
            .set_cell_input(
                target_sheet,
                address(target_sheet, 4, 0),
                CellInput::Text("old document".into()),
            )
            .unwrap();
        let token = target.document().token();
        let before_failed_load = target.document().snapshot();
        let before_failed_stats = target.history_stats();
        assert!(target.load_xlsx(b"not an xlsx package").is_err());
        assert_eq!(target.document().snapshot(), before_failed_load);
        assert_eq!(target.history_stats(), before_failed_stats);
        assert_eq!(target.history_limits(), limits);

        let loaded = target.load_xlsx(&package).unwrap();
        assert_eq!(target.document().token(), token);
        assert_eq!(
            target
                .document()
                .cell_input(source_sheet, address(source_sheet, 0, 0)),
            Some(&CellInput::Text("loaded".into()))
        );
        assert_eq!(loaded.sheets.len(), 1);
        assert!(!target.can_undo());
        assert!(!target.can_redo());
        assert_eq!(target.history_stats(), HistoryStats::default());
        assert_eq!(target.history_limits(), limits);
    }

    #[test]
    fn structural_edits_are_reversible_and_sheet_lifecycle_is_a_barrier() {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        let before = address(sheet, 0, 0);
        api.set_cell_input(sheet, before, CellInput::Text("before".into()))
            .unwrap();
        let stats = api.history_stats();
        api.move_sheet(sheet, 0).unwrap();
        assert_eq!(api.history_stats(), stats);
        assert!(api.can_undo());

        api.insert_rows(sheet, RowIndex(0), 1).unwrap();
        assert!(api.can_undo_sheet(sheet));
        assert!(!api.can_redo());
        assert_eq!(api.history_stats().barriers, stats.barriers);
        let shifted = address(sheet, 1, 0);
        assert_eq!(
            api.document().cell_input(sheet, shifted),
            Some(&CellInput::Text("before".into()))
        );

        api.undo_sheet(sheet).unwrap();
        assert_eq!(
            api.document().cell_input(sheet, before),
            Some(&CellInput::Text("before".into()))
        );
        assert_eq!(api.document().cell_input(sheet, shifted), None);
        api.redo_sheet(sheet).unwrap();
        assert_eq!(
            api.document().cell_input(sheet, shifted),
            Some(&CellInput::Text("before".into()))
        );

        let after_barrier = address(sheet, 0, 0);
        api.set_cell_input(sheet, after_barrier, CellInput::Text("after".into()))
            .unwrap();
        api.undo().unwrap();
        assert_eq!(api.document().cell_input(sheet, after_barrier), None);
        assert_eq!(
            api.document().cell_input(sheet, shifted),
            Some(&CellInput::Text("before".into()))
        );

        // The later same-sheet edit is now in redo. Structural undo remains
        // available in the ordinary reverse chronological sequence.
        api.undo_sheet(sheet).unwrap();
        assert_eq!(
            api.document().cell_input(sheet, before),
            Some(&CellInput::Text("before".into()))
        );
        api.redo_sheet(sheet).unwrap();
        api.redo_sheet(sheet).unwrap();
        assert_eq!(
            api.document().cell_input(sheet, after_barrier),
            Some(&CellInput::Text("after".into()))
        );

        let barrier_count = api.history_stats().barriers;
        api.add_sheet(Some("Structural barrier".into())).unwrap();
        assert!(!api.can_undo());
        assert!(!api.can_redo());
        assert_eq!(api.history_stats().barriers, barrier_count + 1);
    }

    #[test]
    fn structural_history_preserves_sparse_explicit_empty_cells() {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        let original = address(sheet, 4, 3);
        let shifted = address(sheet, 5, 3);
        let snapshot = sheets_core::WorkbookSnapshot {
            revision: sheets_core::Revision(0),
            active_sheet: sheet,
            sheets: vec![sheets_core::SheetSnapshot {
                id: sheet,
                name: "Sheet1".into(),
                cells: vec![sheets_core::CellSnapshot {
                    address: original,
                    input: CellInput::Empty,
                    format: None,
                }],
                merges: Vec::new(),
                row_dimensions: Vec::new(),
                column_dimensions: Vec::new(),
                auto_filter: None,
                manual_hidden_rows: Vec::new(),
                manual_hidden_columns: Vec::new(),
            }],
        };
        api.document.workbook = sheets_core::Workbook::from_snapshot(snapshot).unwrap();
        let original_cells = api.document().snapshot().sheets[0].cells.clone();
        assert_eq!(
            api.document().cell_input(sheet, original),
            Some(&CellInput::Empty)
        );

        api.insert_rows(sheet, RowIndex(0), 1).unwrap();
        assert_eq!(api.history_stats().undo_entries, 1);
        assert_eq!(api.document().cell_input(sheet, original), None);
        assert_eq!(
            api.document().cell_input(sheet, shifted),
            Some(&CellInput::Empty)
        );
        api.undo_sheet(sheet).unwrap();
        assert_eq!(api.document().snapshot().sheets[0].cells, original_cells);
        api.redo_sheet(sheet).unwrap();
        assert_eq!(api.document().cell_input(sheet, original), None);
        assert_eq!(
            api.document().cell_input(sheet, shifted),
            Some(&CellInput::Empty)
        );
    }

    #[test]
    fn shifted_merge_exact_presence_allows_structural_undo_and_redo() {
        for insert in [false, true] {
            let (mut api, _) = test_api();
            let sheet = api.document().active_sheet();
            let original = range(sheet, (2, 0), (4, 1));
            let shifted = if insert {
                range(sheet, (3, 0), (5, 1))
            } else {
                range(sheet, (1, 0), (3, 1))
            };
            api.safe_merge_cells(sheet, original.clone()).unwrap();
            if insert {
                api.insert_rows(sheet, RowIndex(1), 1).unwrap();
            } else {
                api.delete_rows(sheet, RowIndex(1), 1).unwrap();
            }
            assert_eq!(
                api.document().snapshot().sheets[0].merges,
                vec![shifted.clone()]
            );
            api.undo_sheet(sheet).unwrap();
            assert_eq!(api.document().snapshot().sheets[0].merges, vec![original]);
            api.redo_sheet(sheet).unwrap();
            assert_eq!(api.document().snapshot().sheets[0].merges, vec![shifted]);
        }
    }

    #[test]
    fn hidden_columns_are_changed_fields_with_exact_undo_and_redo() {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        let mut changes = ChangeSet::new();
        changes.push(Change::SetManualHiddenColumns {
            sheet,
            columns: vec![ColumnIndex(1), ColumnIndex(3)],
        });
        api.execute(changes).unwrap();
        assert_eq!(
            api.document().snapshot().sheets[0].manual_hidden_columns,
            vec![ColumnIndex(1), ColumnIndex(3)]
        );
        api.undo_sheet(sheet).unwrap();
        assert!(
            api.document().snapshot().sheets[0]
                .manual_hidden_columns
                .is_empty()
        );
        api.redo_sheet(sheet).unwrap();
        assert_eq!(
            api.document().snapshot().sheets[0].manual_hidden_columns,
            vec![ColumnIndex(1), ColumnIndex(3)]
        );
    }

    #[test]
    fn replay_checks_only_changed_cell_fields() {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        let cell = address(sheet, 0, 0);
        api.set_cell_input(sheet, cell, CellInput::Number(2.0))
            .unwrap();
        let mut entry = HistoryEntry {
            sheets: vec![sheet],
            ..HistoryEntry::default()
        };
        entry.cells.push(CellDelta {
            address: cell,
            input: Some(Box::new(Endpoints {
                before: Some(CellInput::Number(1.0)),
                after: Some(CellInput::Number(2.0)),
            })),
            format: None,
        });
        let mut history = History::default();
        history.record(entry);
        assert_eq!(
            history.replay_conflict(true, 0, &api.document.workbook),
            None
        );

        api.set_cell_format(sheet, cell, font_format(true, false), bounds(1))
            .unwrap();
        assert_eq!(
            history.replay_conflict(true, 0, &api.document.workbook),
            None
        );
        api.set_cell_input(sheet, cell, CellInput::Number(3.0))
            .unwrap();
        assert_eq!(
            history.replay_conflict(true, 0, &api.document.workbook),
            Some(sheet)
        );
    }

    #[test]
    fn structural_scope_and_cross_sheet_replay_order_are_preserved() {
        let (mut api, _) = test_api();
        let owner = api.document().active_sheet();
        api.add_sheet(Some("Other".into())).unwrap();
        let foreign = api.document().active_sheet();
        assert_ne!(owner, foreign);

        let mut structural = HistoryEntry {
            origin_revision: Revision(1),
            sheets: vec![owner, foreign],
            ..HistoryEntry::default()
        };
        structural.mark_structural(owner);
        assert_eq!(structural.single_sheet(), Some(owner));
        let mut history = History::default();
        history.record(structural);
        assert_eq!(history.position(true, Some(owner)), Some(0));
        assert_eq!(history.position(true, Some(foreign)), None);

        let unrelated = HistoryEntry {
            origin_revision: Revision(2),
            sheets: vec![foreign],
            ..HistoryEntry::default()
        };
        history.record(unrelated);
        assert_eq!(
            history.replay_conflict(true, 0, &api.document.workbook),
            None
        );

        let mut formula_edit = HistoryEntry {
            origin_revision: Revision(3),
            sheets: vec![foreign],
            ..HistoryEntry::default()
        };
        formula_edit.cells.push(CellDelta {
            address: address(foreign, 0, 0),
            input: Some(Box::new(Endpoints {
                before: None,
                after: Some(CellInput::Formula("Sheet1!A1".into())),
            })),
            format: None,
        });
        history.record(formula_edit);
        assert_eq!(
            history.replay_conflict(true, 0, &api.document.workbook),
            Some(foreign)
        );

        history.did_undo(2);
        assert_eq!(
            history.replay_conflict(true, 0, &api.document.workbook),
            None
        );
        history.did_undo(0);
        assert_eq!(
            history.replay_conflict(false, 0, &api.document.workbook),
            Some(owner)
        );
        assert_eq!(
            history.replay_conflict(false, 1, &api.document.workbook),
            None
        );
    }

    #[test]
    fn successful_new_mutation_after_undo_discards_redo_branch() {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        let a1 = address(sheet, 0, 0);
        let b1 = address(sheet, 0, 1);
        api.set_cell_input(sheet, a1, CellInput::Number(1.0))
            .unwrap();
        api.set_cell_input(sheet, a1, CellInput::Number(2.0))
            .unwrap();
        api.undo().unwrap();
        assert!(api.can_redo());

        api.set_cell_input(sheet, b1, CellInput::Text("new branch".into()))
            .unwrap();
        assert!(!api.can_redo());
        assert_eq!(api.history_stats().redo_entries, 0);
        assert_eq!(
            api.document().cell_input(sheet, a1),
            Some(&CellInput::Number(1.0))
        );
        assert_eq!(
            api.document().cell_input(sheet, b1),
            Some(&CellInput::Text("new branch".into()))
        );
    }

    #[test]
    fn unmerge_of_overlapping_imported_ranges_forms_a_history_barrier() {
        let (mut api, _) = test_api();
        let sheet = api.document().active_sheet();
        let first = range(sheet, (0, 0), (2, 2));
        let overlapping = range(sheet, (1, 1), (3, 3));
        let imported = sheets_core::WorkbookSnapshot {
            revision: sheets_core::Revision(0),
            active_sheet: sheet,
            sheets: vec![sheets_core::SheetSnapshot {
                id: sheet,
                name: "Sheet1".into(),
                cells: Vec::new(),
                merges: vec![first, overlapping],
                row_dimensions: Vec::new(),
                column_dimensions: Vec::new(),
                auto_filter: None,
                manual_hidden_rows: Vec::new(),
                manual_hidden_columns: Vec::new(),
            }],
        };
        api.document.workbook = sheets_core::Workbook::from_snapshot(imported).unwrap();

        let cell = address(sheet, 5, 5);
        api.set_cell_input(sheet, cell, CellInput::Number(9.0))
            .unwrap();
        assert!(api.can_undo());
        let barriers = api.history_stats().barriers;

        api.unmerge_intersecting_selection(sheet, range(sheet, (1, 1), (1, 1)))
            .unwrap();
        assert!(api.document().snapshot().sheets[0].merges.is_empty());
        assert_eq!(
            api.document().cell_input(sheet, cell),
            Some(&CellInput::Number(9.0))
        );
        assert_eq!(api.history_stats().barriers, barriers + 1);
        assert_eq!(api.history_stats().undo_entries, 0);
        assert_eq!(api.history_stats().redo_entries, 0);
        assert!(!api.can_undo());
        assert!(!api.can_redo());
    }
}
