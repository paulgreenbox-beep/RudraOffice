//! Sparse structural transactions. Only formulas pass through IronCalc;
//! canonical formatting and sheet metadata retain their exact source values.

use std::collections::{BTreeMap, BTreeSet};

use sheets_core::SheetSnapshot;
use sheets_ironcalc_adapter::structural_inputs;

use crate::{
    ApiResult, ApiUnsupportedError, AutoFilter, CellAddress, CellFormat, CellInput, CellRange,
    Change, ChangeSet, ColumnIndex, CoreError, EngineResponse, RowIndex, SheetId, SpreadsheetApi,
    SpreadsheetEngine, StructuralOperation,
};

/// Maximum affected source cells and distinct changed cell addresses.
/// Both vacated and destination addresses count, including foreign formulas.
pub const MAX_STRUCTURAL_CELLS: usize = 10_000;
const MAX_NATIVE_ROWS: u32 = 1_048_576;
const MAX_NATIVE_COLUMNS: u32 = 16_384;

#[derive(Clone, Copy)]
struct AxisEdit {
    sheet: SheetId,
    rows: bool,
    insert: bool,
    at: u32,
    count: u32,
    limit: u32,
}

impl AxisEdit {
    fn new(operation: StructuralOperation, rows: u32, columns: u32) -> ApiResult<Self> {
        let (sheet, is_row, insert, at, count) = match operation {
            StructuralOperation::InsertRows { sheet, at, count } => {
                (sheet, true, true, at.0, count)
            }
            StructuralOperation::DeleteRows {
                sheet,
                start,
                count,
            } => (sheet, true, false, start.0, count),
            StructuralOperation::InsertColumns { sheet, at, count } => {
                (sheet, false, true, at.0, count)
            }
            StructuralOperation::DeleteColumns {
                sheet,
                start,
                count,
            } => (sheet, false, false, start.0, count),
        };
        let limit = if is_row {
            rows.min(MAX_NATIVE_ROWS)
        } else {
            columns.min(MAX_NATIVE_COLUMNS)
        };
        if count == 0 || at >= limit || at.checked_add(count).is_none_or(|end| end > limit) {
            return Err(unsupported(
                "zero count, overflow, or axis range exceeds document/native bounds",
            ));
        }
        Ok(Self {
            sheet,
            rows: is_row,
            insert,
            at,
            count,
            limit,
        })
    }

    fn index(self, index: u32) -> ApiResult<Option<u32>> {
        let mapped = if index < self.at {
            Some(index)
        } else if self.insert {
            Some(
                index
                    .checked_add(self.count)
                    .ok_or_else(|| unsupported("shift overflows index space"))?,
            )
        } else if index < self.at + self.count {
            None
        } else {
            Some(index - self.count)
        };
        if mapped.is_some_and(|index| index >= self.limit) {
            return Err(unsupported(
                "shift would move canonical data beyond document/native bounds",
            ));
        }
        Ok(mapped)
    }

    fn address(self, address: CellAddress) -> ApiResult<Option<CellAddress>> {
        if address.sheet != self.sheet {
            return Ok(Some(address));
        }
        let index = if self.rows {
            address.row.0
        } else {
            address.column.0
        };
        Ok(self.index(index)?.map(|index| {
            if self.rows {
                CellAddress::new(address.sheet, RowIndex(index), address.column)
            } else {
                CellAddress::new(address.sheet, address.row, ColumnIndex(index))
            }
        }))
    }

    fn span(self, start: u32, end: u32) -> ApiResult<Option<(u32, u32)>> {
        if self.insert {
            let mapped_start = self.index(start)?.expect("insertion retains every index");
            let mapped_end = self.index(end)?.expect("insertion retains every index");
            return Ok(Some((mapped_start, mapped_end)));
        }
        let deleted_end = self.at + self.count;
        if end < self.at || start >= deleted_end {
            return Ok(Some((
                self.index(start)?.unwrap(),
                self.index(end)?.unwrap(),
            )));
        }
        if start >= self.at && end < deleted_end {
            return Ok(None);
        }
        let mapped_start = if start < self.at { start } else { self.at };
        let mapped_end = if end >= deleted_end {
            end - self.count
        } else {
            self.at - 1
        };
        Ok(Some((mapped_start, mapped_end)))
    }

    fn merge(self, range: &CellRange) -> ApiResult<CellRange> {
        let (start, end) = if self.rows {
            (range.start.row.0, range.end.row.0)
        } else {
            (range.start.column.0, range.end.column.0)
        };
        if (self.insert && start < self.at && self.at <= end)
            || (!self.insert && start < self.at + self.count && self.at <= end)
        {
            return Err(unsupported(
                "edit intersects a merged range; unmerge it first",
            ));
        }
        Ok(CellRange::new(
            self.address(range.start)?
                .expect("nonintersecting merge survives"),
            self.address(range.end)?
                .expect("nonintersecting merge survives"),
        )?)
    }

    fn filter(self, filter: &AutoFilter) -> ApiResult<Option<AutoFilter>> {
        let (start, end) = if self.rows {
            (filter.range.start.row.0, filter.range.end.row.0)
        } else {
            (filter.range.start.column.0, filter.range.end.column.0)
        };
        let Some((start_after, end_after)) = self.span(start, end)? else {
            return Ok(None);
        };
        let mut candidate = filter.clone();
        if self.rows {
            candidate.range.start.row = RowIndex(start_after);
            candidate.range.end.row = RowIndex(end_after);
            // Removing the actual header leaves the surviving rows as data.
            if !self.insert && self.at <= start && start < self.at + self.count {
                candidate.has_header = false;
            }
        } else {
            candidate.range.start.column = ColumnIndex(start_after);
            candidate.range.end.column = ColumnIndex(end_after);
            candidate.criteria = filter
                .criteria
                .iter()
                .filter_map(|(column, criterion)| {
                    self.index(column.0)
                        .transpose()
                        .map(|result| result.map(|column| (ColumnIndex(column), criterion.clone())))
                })
                .collect::<ApiResult<_>>()?;
        }
        candidate.validate()?;
        Ok(Some(candidate))
    }

    fn metadata(self, source: &SheetSnapshot) -> ApiResult<SheetSnapshot> {
        let mut rows = Vec::with_capacity(source.row_dimensions.len());
        for dimension in &source.row_dimensions {
            if let Some(row) = if self.rows {
                self.index(dimension.row.0)?.map(RowIndex)
            } else {
                Some(dimension.row)
            } {
                rows.push(sheets_core::RowDimension {
                    row,
                    height: dimension.height,
                });
            }
        }
        let mut columns = Vec::with_capacity(source.column_dimensions.len());
        for dimension in &source.column_dimensions {
            if let Some(column) = if self.rows {
                Some(dimension.column)
            } else {
                self.index(dimension.column.0)?.map(ColumnIndex)
            } {
                columns.push(sheets_core::ColumnDimension {
                    column,
                    width: dimension.width,
                });
            }
        }
        let manual_hidden_rows = source
            .manual_hidden_rows
            .iter()
            .filter_map(|row| {
                if self.rows {
                    self.index(row.0)
                        .transpose()
                        .map(|result| result.map(RowIndex))
                } else {
                    Some(Ok(*row))
                }
            })
            .collect::<ApiResult<_>>()?;
        let manual_hidden_columns = source
            .manual_hidden_columns
            .iter()
            .filter_map(|column| {
                if self.rows {
                    Some(Ok(*column))
                } else {
                    self.index(column.0)
                        .transpose()
                        .map(|result| result.map(ColumnIndex))
                }
            })
            .collect::<ApiResult<_>>()?;
        Ok(SheetSnapshot {
            id: source.id,
            name: source.name.clone(),
            cells: Vec::new(),
            merges: source
                .merges
                .iter()
                .map(|range| self.merge(range))
                .collect::<ApiResult<_>>()?,
            row_dimensions: rows,
            column_dimensions: columns,
            auto_filter: source
                .auto_filter
                .as_ref()
                .map(|filter| self.filter(filter))
                .transpose()?
                .flatten(),
            manual_hidden_rows,
            manual_hidden_columns,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CellFields {
    pub(crate) input: Option<CellInput>,
    pub(crate) format: Option<CellFormat>,
}

fn unsupported(reason: impl Into<String>) -> crate::ApiError {
    ApiUnsupportedError::StructuralEdit(reason.into()).into()
}

impl<E: SpreadsheetEngine> SpreadsheetApi<E> {
    /// Manual visibility is canonical and independent of auto-filter visibility.
    pub fn set_manual_hidden_columns(
        &mut self,
        sheet: SheetId,
        columns: Vec<ColumnIndex>,
    ) -> ApiResult<EngineResponse> {
        self.execute(ChangeSet::new().with(Change::SetManualHiddenColumns { sheet, columns }))
    }

    pub fn manual_hidden_columns(&self, sheet: SheetId) -> Vec<ColumnIndex> {
        self.document
            .workbook
            .manual_hidden_columns(sheet)
            .map(|columns| columns.iter().copied().collect())
            .unwrap_or_default()
    }

    pub(crate) fn execute_structural(
        &mut self,
        operation: StructuralOperation,
    ) -> ApiResult<EngineResponse> {
        self.config.validate()?;
        let edit = AxisEdit::new(
            operation,
            self.config.max_document_rows,
            self.config.max_document_columns,
        )?;
        let before = self.document.snapshot();
        self.config.validate_snapshot(&before)?;
        let source = before
            .sheets
            .iter()
            .find(|sheet| sheet.id == edit.sheet)
            .ok_or(CoreError::SheetNotFound(edit.sheet))?;
        let metadata = edit.metadata(source)?;
        // Reject a large affected tail before constructing the native candidate.
        let affected = source
            .cells
            .iter()
            .filter(|cell| {
                if edit.rows {
                    cell.address.row.0 >= edit.at
                } else {
                    cell.address.column.0 >= edit.at
                }
            })
            .count();
        if affected > MAX_STRUCTURAL_CELLS {
            return Err(unsupported(
                "affected materialized cells exceed the 10,000-address bound",
            ));
        }
        let native = structural_inputs(&before, &operation)
            .map_err(|error| unsupported(error.to_string()))?;
        let mut original = BTreeMap::new();
        let mut candidate = BTreeMap::new();
        for sheet in &before.sheets {
            for cell in &sheet.cells {
                let fields = CellFields {
                    input: self
                        .document
                        .workbook
                        .cell_input(sheet.id, cell.address)
                        .cloned(),
                    format: cell.format.clone(),
                };
                original.insert(cell.address, fields.clone());
                if let Some(address) = edit.address(cell.address)? {
                    let input = if matches!(fields.input, Some(CellInput::Formula(_))) {
                        Some(
                            native
                                .get(&address)
                                .ok_or_else(|| unsupported("native candidate lost a formula"))?
                                .clone(),
                        )
                    } else {
                        fields.input
                    };
                    candidate.insert(
                        address,
                        CellFields {
                            input,
                            format: fields.format,
                        },
                    );
                }
            }
        }
        let addresses: BTreeSet<_> = original.keys().chain(candidate.keys()).copied().collect();
        let mut changes = ChangeSet::new();
        // Topology must be removed before shifted content and replacement merges.
        for range in &source.merges {
            if !metadata.merges.contains(range) {
                changes.push(Change::UnmergeCells {
                    sheet: edit.sheet,
                    range: range.clone(),
                });
            }
        }
        let empty = CellFields::default();
        let mut changed = 0usize;
        for address in addresses {
            let current = original.get(&address).unwrap_or(&empty);
            let target = candidate.get(&address).unwrap_or(&empty);
            if current != target {
                changed += 1;
                if changed > MAX_STRUCTURAL_CELLS {
                    return Err(unsupported(
                        "changed source/destination addresses exceed the 10,000-address bound",
                    ));
                }
                replace_cell(&mut changes, address, target);
            }
        }
        metadata_changes(&mut changes, source, &metadata);
        for range in &metadata.merges {
            if !source.merges.contains(range) {
                changes.push(Change::MergeCells {
                    sheet: edit.sheet,
                    range: range.clone(),
                });
            }
        }
        self.execute_with_history_policy(changes, true, Some(edit.sheet))
    }
}

pub(crate) fn replace_cell(changes: &mut ChangeSet, address: CellAddress, target: &CellFields) {
    let sheet = address.sheet;
    changes.push(Change::ClearCellFormat { sheet, address });
    changes.push(Change::SetCellInput {
        sheet,
        address,
        input: CellInput::Empty,
    });
    match &target.input {
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
                    input: input.clone(),
                });
            }
            if let Some(format) = &target.format {
                changes.push(Change::SetCellFormat {
                    sheet,
                    address,
                    format: format.clone(),
                });
            }
        }
    }
}

fn metadata_changes(changes: &mut ChangeSet, before: &SheetSnapshot, after: &SheetSnapshot) {
    let sheet = before.id;
    let rows_before: BTreeMap<_, _> = before
        .row_dimensions
        .iter()
        .map(|d| (d.row, d.height))
        .collect();
    let rows_after: BTreeMap<_, _> = after
        .row_dimensions
        .iter()
        .map(|d| (d.row, d.height))
        .collect();
    for row in rows_before
        .keys()
        .chain(rows_after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        if rows_before.get(&row) != rows_after.get(&row) {
            changes.push(match rows_after.get(&row) {
                Some(height) => Change::SetRowDimension {
                    sheet,
                    row,
                    height: *height,
                },
                None => Change::ClearRowDimension { sheet, row },
            });
        }
    }
    let columns_before: BTreeMap<_, _> = before
        .column_dimensions
        .iter()
        .map(|d| (d.column, d.width))
        .collect();
    let columns_after: BTreeMap<_, _> = after
        .column_dimensions
        .iter()
        .map(|d| (d.column, d.width))
        .collect();
    for column in columns_before
        .keys()
        .chain(columns_after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        if columns_before.get(&column) != columns_after.get(&column) {
            changes.push(match columns_after.get(&column) {
                Some(width) => Change::SetColumnDimension {
                    sheet,
                    column,
                    width: *width,
                },
                None => Change::ClearColumnDimension { sheet, column },
            });
        }
    }
    if before.auto_filter != after.auto_filter {
        changes.push(Change::SetAutoFilter {
            sheet,
            filter: after.auto_filter.clone(),
        });
    }
    if before.manual_hidden_rows != after.manual_hidden_rows {
        changes.push(Change::SetManualHiddenRows {
            sheet,
            rows: after.manual_hidden_rows.clone(),
        });
    }
    if before.manual_hidden_columns != after.manual_hidden_columns {
        changes.push(Change::SetManualHiddenColumns {
            sheet,
            columns: after.manual_hidden_columns.clone(),
        });
    }
}
