//! Canonical bounded sorting and filter definitions; visibility is derived.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::*;
use sheets_core::{AutoFilter, CellError, FilterCriterion};

pub const MAX_SORT_FILTER_CELLS: u64 = 10_000;
/// Limits criteria retained in canonical state independently of history budgets.
pub const MAX_FILTER_TEXT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortOptions {
    pub range: CellRange,
    pub key_column: ColumnIndex,
    pub ascending: bool,
    pub has_header: bool,
}

pub(crate) fn validate_filter_payload(filter: &AutoFilter) -> ApiResult<()> {
    filter.validate()?;
    validate_rectangle_cap(&filter.range)?;
    let mut bytes = 0usize;
    let mut value_entries = 0usize;
    for criterion in filter.criteria.values() {
        let texts: &[String] = match criterion {
            FilterCriterion::Values(values) => {
                if values.len() > MAX_SORT_FILTER_CELLS as usize {
                    return Err(ApiTransactionError::DocumentLimitExceeded(
                        "too many filter values".into(),
                    )
                    .into());
                }
                values
            }
            FilterCriterion::Equals(text)
            | FilterCriterion::NotEquals(text)
            | FilterCriterion::Contains(text) => std::slice::from_ref(text),
            _ => &[],
        };
        value_entries = value_entries.checked_add(texts.len()).ok_or_else(|| {
            ApiTransactionError::DocumentLimitExceeded("filter value count overflow".into())
        })?;
        if value_entries > MAX_SORT_FILTER_CELLS as usize {
            return Err(ApiTransactionError::DocumentLimitExceeded(
                "aggregate filter values exceed 10000 entries".into(),
            )
            .into());
        }
        for text in texts {
            bytes = bytes.checked_add(text.len()).ok_or_else(|| {
                ApiTransactionError::DocumentLimitExceeded("filter text size overflow".into())
            })?;
            if bytes > MAX_FILTER_TEXT_BYTES {
                return Err(ApiTransactionError::DocumentLimitExceeded(
                    "filter text exceeds 1 MiB".into(),
                )
                .into());
            }
        }
    }
    Ok(())
}

fn validate_rectangle_cap(range: &CellRange) -> ApiResult<()> {
    range.validate()?;
    let cells = range
        .rows()
        .checked_mul(range.cols())
        .ok_or_else(|| CoreError::InvalidRange("range area overflow".into()))?;
    if cells > MAX_SORT_FILTER_CELLS {
        return Err(ApiTransactionError::DocumentLimitExceeded(format!(
            "sort/filter range contains {cells} cells, exceeding {MAX_SORT_FILTER_CELLS}"
        ))
        .into());
    }
    Ok(())
}

impl<E: SpreadsheetEngine> SpreadsheetApi<E> {
    /// Exact canonical definition, without a derived hidden-row cache.
    pub fn auto_filter(&self, sheet: SheetId) -> Option<&AutoFilter> {
        self.document.workbook.auto_filter(sheet)
    }

    pub fn set_auto_filter(
        &mut self,
        sheet: SheetId,
        filter: AutoFilter,
    ) -> ApiResult<EngineResponse> {
        self.validate_sheet_range(sheet, &filter.range)?;
        validate_filter_payload(&filter)?;
        self.execute_reversible(ChangeSet::new().with(Change::SetAutoFilter {
            sheet,
            filter: Some(filter),
        }))
    }

    /// Removes criteria/range only; manual hiding and cell data are unchanged.
    pub fn clear_auto_filter(&mut self, sheet: SheetId) -> ApiResult<EngineResponse> {
        self.execute_reversible(ChangeSet::new().with(Change::SetAutoFilter {
            sheet,
            filter: None,
        }))
    }

    pub fn set_manual_hidden_rows(
        &mut self,
        sheet: SheetId,
        rows: Vec<RowIndex>,
    ) -> ApiResult<EngineResponse> {
        self.execute(ChangeSet::new().with(Change::SetManualHiddenRows { sheet, rows }))
    }

    pub fn manual_hidden_rows(&self, sheet: SheetId) -> Vec<RowIndex> {
        self.document
            .workbook
            .manual_hidden_rows(sheet)
            .map(|rows| rows.iter().copied().collect())
            .unwrap_or_default()
    }

    /// AND across columns, OR within Values. Text comparisons are case-sensitive
    /// Unicode. Values uses unformatted canonical value text (numeric 5 and text
    /// "5" deliberately match); numeric predicates never coerce text. Blank also
    /// includes an evaluated empty string. Header rows bypass filter criteria.
    /// Unknown sheets/no definition/empty criteria produce no filtered rows.
    pub fn filter_hidden_rows(&self, sheet: SheetId) -> Vec<RowIndex> {
        let Some(filter) = self.auto_filter(sheet) else {
            return Vec::new();
        };
        if filter.criteria.is_empty() {
            return Vec::new();
        }
        let first = filter.range.start.row.0 + u32::from(filter.has_header);
        (first..=filter.range.end.row.0)
            .filter_map(|row| {
                let accepted = filter.criteria.iter().all(|(column, criterion)| {
                    let value = self.evaluated_filter_value(CellAddress::new(
                        sheet,
                        RowIndex(row),
                        *column,
                    ));
                    matches_criterion(&value, criterion)
                });
                (!accepted).then_some(RowIndex(row))
            })
            .collect()
    }

    /// Union for projection; clearing a filter never unhides a manually hidden row.
    pub fn hidden_rows(&self, sheet: SheetId) -> Vec<RowIndex> {
        self.manual_hidden_rows(sheet)
            .into_iter()
            .chain(self.filter_hidden_rows(sheet))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Enumerates only the bounded canonical filter rectangle, not a source window.
    pub fn filter_unique_values(
        &self,
        sheet: SheetId,
        column: ColumnIndex,
    ) -> ApiResult<Vec<String>> {
        let filter = self
            .auto_filter(sheet)
            .ok_or_else(|| CoreError::InvalidRange("sheet has no auto-filter".into()))?;
        self.validate_sheet_range(sheet, &filter.range)?;
        validate_filter_payload(filter)?;
        if column < filter.range.start.column || column > filter.range.end.column {
            return Err(
                CoreError::InvalidRange("filter column is outside its range".into()).into(),
            );
        }
        let first = filter.range.start.row.0 + u32::from(filter.has_header);
        let mut values = BTreeSet::new();
        let mut bytes = 0usize;
        for row in first..=filter.range.end.row.0 {
            let value = value_text(&self.evaluated_filter_value(CellAddress::new(
                sheet,
                RowIndex(row),
                column,
            )));
            if !values.contains(&value) {
                bytes = bytes.checked_add(value.len()).ok_or_else(|| {
                    ApiTransactionError::DocumentLimitExceeded(
                        "unique filter values overflow".into(),
                    )
                })?;
                if bytes > MAX_FILTER_TEXT_BYTES {
                    return Err(ApiTransactionError::DocumentLimitExceeded(
                        "unique filter value text exceeds 1 MiB".into(),
                    )
                    .into());
                }
                values.insert(value);
            }
        }
        Ok(values.into_iter().collect())
    }

    fn evaluated_filter_value(&self, address: CellAddress) -> CellValue {
        if let Some(value) = self.document.cell_value(address.sheet, address) {
            return value.clone();
        }
        match self.document.cell_input(address.sheet, address) {
            Some(CellInput::Formula(_)) => CellValue::Error(CellError::Value),
            Some(input) => cell_input_into_core_value(input.clone()).unwrap_or(CellValue::Empty),
            None => CellValue::Empty,
        }
    }

    /// Stable sort of the entire selected rectangle, including filtered/manual
    /// hidden rows. It does not expand a single cell to an inferred table.
    /// Header is explicit; physical row sizes/hiding remain at their row indices.
    /// Numbers (including serial dates/times/percent), text, booleans and errors
    /// have fixed group order. Direction reverses values within each group;
    /// blanks are last in both directions. Text uses Unicode order, not locale
    /// collation. Moved formulas use IronCalc copy-relative relocation; formulas
    /// outside the rectangle retain their address-based references unchanged.
    pub fn sort_range(
        &mut self,
        sheet: SheetId,
        options: SortOptions,
    ) -> ApiResult<EngineResponse> {
        let range = &options.range;
        self.validate_sheet_range(sheet, range)?;
        validate_rectangle_cap(range)?;
        if options.key_column < range.start.column || options.key_column > range.end.column {
            return Err(
                CoreError::InvalidRange("sort key column is outside its range".into()).into(),
            );
        }
        let snapshot = self.document.snapshot();
        let sheet_snapshot = snapshot
            .sheets
            .iter()
            .find(|s| s.id == sheet)
            .ok_or(CoreError::SheetNotFound(sheet))?;
        if let Some(merge) = sheet_snapshot
            .merges
            .iter()
            .find(|merge| merge.intersects(range))
        {
            return Err(ApiUnsupportedError::SortMergedRange {
                range: merge.clone(),
            }
            .into());
        }
        let first = range.start.row.0 + u32::from(options.has_header);
        for row in first..=range.end.row.0 {
            let address = CellAddress::new(sheet, RowIndex(row), options.key_column);
            if self
                .document
                .cell_input(sheet, address)
                .is_some_and(CellInput::is_formula)
                && self.document.cell_value(sheet, address).is_none()
            {
                return Err(ApiUnsupportedError::SortFormula(
                    "sort key formula has no calculated value".into(),
                )
                .into());
            }
        }
        let mut rows: Vec<_> = (first..=range.end.row.0)
            .map(|row| {
                let address = CellAddress::new(sheet, RowIndex(row), options.key_column);
                (RowIndex(row), self.evaluated_filter_value(address))
            })
            .collect();
        rows.sort_by(|(_, left), (_, right)| compare_values(left, right, options.ascending));
        let moves: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(offset, (source, _))| (*source, RowIndex(first + offset as u32)))
            .collect();
        if moves
            .iter()
            .all(|(source, destination)| source == destination)
        {
            return Ok(EngineResponse::new(self.document.revision(), Vec::new()));
        }
        let translated =
            sheets_ironcalc_adapter::translate_sorted_formulas(&snapshot, range, &moves)
                .map_err(|error| ApiUnsupportedError::SortFormula(error.to_string()))?;
        let cells: BTreeMap<_, _> = self
            .document
            .query_range(range)
            .into_iter()
            .map(|cell| ((cell.address.row, cell.address.column), cell))
            .collect();
        let mut changes = ChangeSet::new();
        for (source, destination) in moves {
            if source == destination {
                continue;
            }
            for column in range.start.column.0..=range.end.column.0 {
                let column = ColumnIndex(column);
                let address = CellAddress::new(sheet, destination, column);
                // Clear both fields so an absent source cannot inherit stale
                // destination formatting. The immutable map keeps row integrity.
                changes.push(Change::ClearCellFormat { sheet, address });
                changes.push(Change::SetCellInput {
                    sheet,
                    address,
                    input: CellInput::Empty,
                });
                if let Some(cell) = cells.get(&(source, column)) {
                    let source_input = self.document.cell_input(sheet, cell.address);
                    if matches!(source_input, Some(CellInput::Empty)) {
                        changes.push(Change::SetCellFormat {
                            sheet,
                            address,
                            format: cell.format.clone().unwrap_or_default(),
                        });
                        changes.push(Change::SetCellInput {
                            sheet,
                            address,
                            input: CellInput::Empty,
                        });
                        if cell.format.is_none() {
                            changes.push(Change::ClearCellFormat { sheet, address });
                        }
                        continue;
                    }
                    if let Some(format) = &cell.format {
                        changes.push(Change::SetCellFormat {
                            sheet,
                            address,
                            format: format.clone(),
                        });
                    }
                    let input = match &cell.input {
                        CellInput::Formula(_) => CellInput::formula(
                            translated
                                .get(&address)
                                .ok_or_else(|| {
                                    ApiUnsupportedError::SortFormula(
                                        "translator omitted a moved formula".into(),
                                    )
                                })?
                                .clone(),
                        )?,
                        input => input.clone(),
                    };
                    if input != CellInput::Empty {
                        changes.push(Change::SetCellInput {
                            sheet,
                            address,
                            input,
                        });
                    }
                }
            }
        }
        self.execute_reversible(changes)
    }
}

fn numeric(value: &CellValue) -> Option<f64> {
    match value {
        CellValue::Number(v) | CellValue::Percent(v) | CellValue::Date(v) | CellValue::Time(v) => {
            Some(*v)
        }
        _ => None,
    }
}

fn blank(value: &CellValue) -> bool {
    matches!(value, CellValue::Empty) || matches!(value, CellValue::Text(text) if text.is_empty())
}

fn value_text(value: &CellValue) -> String {
    match value {
        CellValue::Empty => String::new(),
        CellValue::Text(text) => text.clone(),
        CellValue::Number(value) | CellValue::Date(value) | CellValue::Time(value) => {
            value.to_string()
        }
        CellValue::Percent(value) => format!("{value}%"),
        CellValue::Boolean(value) => value.to_string(),
        CellValue::Error(error) => error.to_string(),
    }
}

fn matches_criterion(value: &CellValue, criterion: &FilterCriterion) -> bool {
    match criterion {
        FilterCriterion::Values(values) => values.contains(&value_text(value)),
        FilterCriterion::Equals(text) => value_text(value) == *text,
        FilterCriterion::NotEquals(text) => value_text(value) != *text,
        FilterCriterion::Contains(text) => value_text(value).contains(text),
        FilterCriterion::Blank => blank(value),
        FilterCriterion::NonBlank => !blank(value),
        FilterCriterion::NumberEquals(expected) => {
            numeric(value).is_some_and(|value| value == *expected)
        }
        FilterCriterion::GreaterThan(expected) => {
            numeric(value).is_some_and(|value| value > *expected)
        }
        FilterCriterion::LessThan(expected) => {
            numeric(value).is_some_and(|value| value < *expected)
        }
    }
}

fn compare_values(left: &CellValue, right: &CellValue, ascending: bool) -> Ordering {
    let rank = |value: &CellValue| {
        if blank(value) {
            4
        } else if numeric(value).is_some() {
            0
        } else {
            match value {
                CellValue::Text(_) => 1,
                CellValue::Boolean(_) => 2,
                _ => 3,
            }
        }
    };
    let group = rank(left).cmp(&rank(right));
    if group != Ordering::Equal {
        return group;
    }
    if blank(left) {
        return Ordering::Equal;
    }
    let comparison = match (left, right) {
        (a, b) if numeric(a).is_some() && numeric(b).is_some() => numeric(a)
            .unwrap()
            .partial_cmp(&numeric(b).unwrap())
            .unwrap_or(Ordering::Equal),
        (CellValue::Text(a), CellValue::Text(b)) => a.cmp(b),
        (CellValue::Boolean(a), CellValue::Boolean(b)) => a.cmp(b),
        (a, b) => value_text(a).cmp(&value_text(b)),
    };
    if ascending {
        comparison
    } else {
        comparison.reverse()
    }
}
