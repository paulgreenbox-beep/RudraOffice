//! Bounded immutable cell clipboard and exact, document-owned transactions.
//! Coordinates are logical: hidden and filtered rows remain in the rectangle.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::mem::size_of;

use sheets_core::{NumberFormat, Revision};
use sheets_ironcalc_adapter::{
    build_native_cut_model, copied_formula_inputs, extract_native_cut_formulas,
    moved_formula_inputs,
};

use crate::clipboard_tsv::{parse_tsv, serialize_tsv};
use crate::structural::{CellFields, replace_cell};
use crate::{
    ApiResult, ApiUnsupportedError, CellAddress, CellFormat, CellInput, CellRange, CellSnapshot,
    CellValue, Change, ChangeSet, ColumnIndex, EntryParser, MAX_CLIPBOARD_CELLS,
    MAX_CLIPBOARD_TSV_BYTES, RowIndex, SpreadsheetApi, SpreadsheetEngine,
};

/// Rich data, strings, metadata, and the owned TSV are bounded together.
pub const MAX_CLIPBOARD_PAYLOAD_BYTES: usize = 4 * 1024 * 1024;

const MAX_NATIVE_CLIPBOARD_WIRE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct NativeClipboardWire(Vec<u8>);

impl Write for NativeClipboardWire {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_NATIVE_CLIPBOARD_WIRE_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other("native clipboard wire exceeds 8 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct CapturedCell {
    address: CellAddress,
    fields: CellFields,
}

/// Immutable runtime capture. Absence, explicit empty, and formatted blank are
/// preserved separately. Row/column dimensions and whole books are not retained.
#[derive(Clone, Debug)]
pub struct ClipboardPayload {
    source_token: u64,
    source_revision: Revision,
    source_epoch: u64,
    source_range: CellRange,
    source_sheet_name: String,
    sheet_names: Vec<String>,
    cells: Vec<CapturedCell>,
    merges: Vec<CellRange>,
    tsv: String,
}

impl ClipboardPayload {
    pub fn tsv(&self) -> &str {
        &self.tsv
    }
    pub fn source_token(&self) -> u64 {
        self.source_token
    }
    pub fn source_revision(&self) -> Revision {
        self.source_revision
    }
    pub fn source_range(&self) -> &CellRange {
        &self.source_range
    }
    pub fn source_sheet_name(&self) -> &str {
        &self.source_sheet_name
    }
    pub fn rows(&self) -> u32 {
        self.source_range.rows() as u32
    }
    pub fn columns(&self) -> u32 {
        self.source_range.cols() as u32
    }
    pub fn materialized_cells(&self) -> usize {
        self.cells.len()
    }
    pub fn estimated_owned_bytes(&self) -> usize {
        size_of::<Self>()
            + self.cells.capacity() * size_of::<CapturedCell>()
            + self
                .cells
                .iter()
                .map(|cell| fields_bytes(&cell.fields))
                .sum::<usize>()
            + self.merges.capacity() * size_of::<CellRange>()
            + self.sheet_names.capacity() * size_of::<String>()
            + self.sheet_names.iter().map(String::capacity).sum::<usize>()
            + self.source_sheet_name.capacity()
            + self.tsv.capacity()
    }
}

fn error(reason: impl Into<String>) -> crate::ApiError {
    ApiUnsupportedError::Clipboard(reason.into()).into()
}

fn format_bytes(format: &CellFormat) -> usize {
    format.font.family.len()
        + match &format.number_format {
            NumberFormat::Custom(text) | NumberFormat::Currency { symbol: text, .. } => text.len(),
            _ => 0,
        }
}

fn fields_bytes(fields: &CellFields) -> usize {
    input_bytes(fields.input.as_ref()) + fields.format.as_ref().map(format_bytes).unwrap_or(0)
}

fn input_bytes(input: Option<&CellInput>) -> usize {
    match input {
        Some(CellInput::Text(text) | CellInput::Formula(text)) => text.len(),
        _ => 0,
    }
}

fn area(range: &CellRange) -> ApiResult<usize> {
    range.validate()?;
    let area = range
        .rows()
        .checked_mul(range.cols())
        .ok_or_else(|| error("rectangle size overflow"))?;
    if area > MAX_CLIPBOARD_CELLS as u64 {
        return Err(error("rectangle exceeds 10,000 logical cells"));
    }
    Ok(area as usize)
}

fn addresses(range: &CellRange) -> impl Iterator<Item = CellAddress> + '_ {
    (range.start.row.0..=range.end.row.0).flat_map(move |row| {
        (range.start.column.0..=range.end.column.0)
            .map(move |column| CellAddress::new(range.sheet, RowIndex(row), ColumnIndex(column)))
    })
}

fn contains_range(outer: &CellRange, inner: &CellRange) -> bool {
    outer.contains(&inner.start) && outer.contains(&inner.end)
}

fn map_address(source: &CellRange, target: &CellRange, address: CellAddress) -> CellAddress {
    CellAddress::new(
        target.sheet,
        RowIndex(target.start.row.0 + address.row.0 - source.start.row.0),
        ColumnIndex(target.start.column.0 + address.column.0 - source.start.column.0),
    )
}

impl<E: SpreadsheetEngine> SpreadsheetApi<E> {
    /// Copy is a read-only capture and does not discard redo. All logical rows,
    /// including hidden rows, participate. The consumer checks source availability.
    pub fn copy_range(&self, range: CellRange) -> ApiResult<ClipboardPayload> {
        self.validate_sheet_range(range.sheet, &range)?;
        let count = area(&range)?;
        // Measure borrowed values before cloning selected inputs or styles.
        let mut owned = size_of::<ClipboardPayload>().saturating_add(
            self.document
                .workbook
                .sheet_count()
                .saturating_mul(size_of::<String>()),
        );
        for (_, name) in self.document.workbook.sheet_names() {
            owned = owned.saturating_add(name.len());
        }
        owned = owned.saturating_add(
            self.document
                .workbook
                .sheet_name(range.sheet)
                .unwrap()
                .len(),
        );
        if owned > MAX_CLIPBOARD_PAYLOAD_BYTES {
            return Err(error("clipboard sheet metadata exceeds 4 MiB"));
        }
        let mut materialized = 0usize;
        for address in addresses(&range) {
            let input = self.document.workbook.cell_input(range.sheet, address);
            let format = self.document.workbook.cell_format(range.sheet, address);
            if input.is_some() || format.is_some() {
                materialized += 1;
                owned = owned
                    .saturating_add(size_of::<CapturedCell>())
                    .saturating_add(input_bytes(input))
                    .saturating_add(format.map(format_bytes).unwrap_or(0));
                if owned > MAX_CLIPBOARD_PAYLOAD_BYTES {
                    return Err(error("rich payload exceeds 4 MiB"));
                }
            }
            if let Some(CellValue::Text(text)) = self.document.cell_value(range.sheet, address)
                && text.len() > MAX_CLIPBOARD_TSV_BYTES
            {
                return Err(error("displayed cell exceeds TSV text bound"));
            }
        }
        let mut cells = Vec::with_capacity(materialized);
        let mut matrix = Vec::with_capacity(range.rows() as usize);
        let mut text_bytes = 0usize;
        for row in range.start.row.0..=range.end.row.0 {
            let mut record = Vec::with_capacity(range.cols() as usize);
            for column in range.start.column.0..=range.end.column.0 {
                let address = CellAddress::new(range.sheet, RowIndex(row), ColumnIndex(column));
                let input = self.document.cell_input(range.sheet, address);
                let format = self.document.cell_format(range.sheet, address);
                let value = if matches!(input, Some(CellInput::Formula(_))) {
                    match self.document.cell_value(range.sheet, address) {
                        Some(CellValue::Empty) => String::new(),
                        Some(_) => self.cell_view(range.sheet, address)?.display_text(),
                        None => return Err(error("formula has no evaluated value for text copy")),
                    }
                } else {
                    self.cell_view(range.sheet, address)?.display_text()
                };
                text_bytes = text_bytes.saturating_add(value.len());
                if text_bytes > MAX_CLIPBOARD_TSV_BYTES {
                    return Err(error("displayed text exceeds 1 MiB"));
                }
                record.push(value);
                if input.is_some() || format.is_some() {
                    cells.push(CapturedCell {
                        address,
                        fields: CellFields {
                            input: input.cloned(),
                            format: format.cloned(),
                        },
                    });
                }
            }
            matrix.push(record);
        }
        debug_assert_eq!(matrix.iter().map(Vec::len).sum::<usize>(), count);
        let tsv = serialize_tsv(&matrix)?;
        // Only names and intersecting merge topology enter the owned clipboard.
        let mut merges = Vec::new();
        for merge in self
            .document
            .workbook
            .sheet_merges(range.sheet)
            .expect("validated sheet")
        {
            if merge.intersects(&range) {
                if !contains_range(&range, merge) {
                    return Err(error("copy partially intersects a merge"));
                }
                if merges
                    .iter()
                    .any(|other: &CellRange| other.intersects(merge))
                {
                    return Err(error("overlapping imported merge topology is unsupported"));
                }
                merges.push(merge.clone());
            }
        }
        let source_sheet_name = self
            .document
            .workbook
            .sheet_name(range.sheet)
            .unwrap()
            .to_owned();
        let payload = ClipboardPayload {
            source_token: self.document.token(),
            source_revision: self.document.revision(),
            source_epoch: self.clipboard_epoch,
            source_range: range,
            source_sheet_name,
            sheet_names: self
                .document
                .workbook
                .sheet_names()
                .map(|(_, name)| name.to_owned())
                .collect(),
            cells,
            merges,
            tsv,
        };
        if payload.estimated_owned_bytes() > MAX_CLIPBOARD_PAYLOAD_BYTES {
            return Err(error("owned clipboard exceeds 4 MiB"));
        }
        Ok(payload)
    }

    /// Pure navigation may change Revision without changing this content epoch.
    /// Any content edit, history replay, or successful load invalidates a cut.
    pub fn cut_payload_valid(&self, payload: &ClipboardPayload) -> bool {
        payload.source_token == self.document.token()
            && payload.source_epoch == self.clipboard_epoch
            && self
                .document
                .workbook
                .sheet_name(payload.source_range.sheet)
                == Some(payload.source_sheet_name.as_str())
    }

    fn clipboard_destination(
        &self,
        selection: CellRange,
        rows: u32,
        columns: u32,
    ) -> ApiResult<CellRange> {
        self.validate_sheet_range(selection.sheet, &selection)?;
        area(&selection)?;
        if (selection.rows() != 1 || selection.cols() != 1)
            && (selection.rows() != u64::from(rows) || selection.cols() != u64::from(columns))
        {
            return Err(error(
                "destination must be one cell or exactly the clipboard shape",
            ));
        }
        let end_row = selection
            .start
            .row
            .0
            .checked_add(rows - 1)
            .ok_or_else(|| error("destination row overflow"))?;
        let end_column = selection
            .start
            .column
            .0
            .checked_add(columns - 1)
            .ok_or_else(|| error("destination column overflow"))?;
        let target = CellRange::new(
            selection.start,
            CellAddress::new(selection.sheet, RowIndex(end_row), ColumnIndex(end_column)),
        )?;
        self.validate_sheet_range(target.sheet, &target)?;
        area(&target)?;
        if end_row >= 1_048_576 || end_column >= 16_384 {
            return Err(error("destination exceeds native grid bounds"));
        }
        Ok(target)
    }

    fn clipboard_collision(&self, target: &CellRange) -> ApiResult<()> {
        if self
            .document
            .workbook
            .sheet_merges(target.sheet)
            .is_some_and(|merges| merges.iter().any(|merge| merge.intersects(target)))
        {
            return Err(error("destination intersects an existing merge"));
        }
        Ok(())
    }

    pub(crate) fn clipboard_workbook_preflight(&self) -> ApiResult<()> {
        let usage = self.document.workbook.resource_usage();
        if usage.sheets > 1_024
            || usage.records > 100_000
            || usage.formulas > 10_000
            || usage.estimated_owned_bytes > 16 * 1024 * 1024
        {
            return Err(error(
                "clipboard transaction exceeds bounded canonical workbook preparation",
            ));
        }
        Ok(())
    }

    fn clipboard_formula_names(&self) -> ApiResult<Vec<String>> {
        let count = self.document.workbook.sheet_count();
        let bytes = self
            .document
            .workbook
            .sheet_names()
            .fold(0usize, |bytes, (_, name)| bytes.saturating_add(name.len()));
        if count > 1_024 || bytes > MAX_CLIPBOARD_PAYLOAD_BYTES {
            return Err(error("clipboard formula sheet context exceeds budget"));
        }
        Ok(self
            .document
            .workbook
            .sheet_names()
            .map(|(_, name)| name.to_owned())
            .collect())
    }

    /// A single reversible overwrite; immutable source capture makes overlap safe.
    pub fn paste_range(
        &mut self,
        payload: &ClipboardPayload,
        selection: CellRange,
    ) -> ApiResult<CellRange> {
        let target = self.clipboard_destination(selection, payload.rows(), payload.columns())?;
        self.clipboard_collision(&target)?;
        self.clipboard_workbook_preflight()?;
        let cells = payload
            .cells
            .iter()
            .map(|cell| CellSnapshot {
                address: cell.address,
                input: cell.fields.input.clone().unwrap_or(CellInput::Empty),
                format: None,
            })
            .collect::<Vec<_>>();
        let target_name = self
            .document
            .workbook
            .sheet_name(target.sheet)
            .expect("validated sheet");
        let same_workbook = payload.source_token == self.document.token();
        let mut captured_names = payload.sheet_names.clone();
        if same_workbook && !captured_names.iter().any(|name| name == target_name) {
            captured_names.push(target_name.to_owned());
        }
        let mut formulas = copied_formula_inputs(
            &captured_names,
            &payload.source_sheet_name,
            &payload.source_range,
            &cells,
            target.start,
            target_name,
            same_workbook,
        )
        .map_err(|native| error(native.to_string()))?;
        if same_workbook && !formulas.is_empty() {
            let current_names = self.clipboard_formula_names()?;
            if current_names != captured_names {
                let relocated = formulas
                    .iter()
                    .map(|(address, input)| CellSnapshot {
                        address: *address,
                        input: input.clone(),
                        format: None,
                    })
                    .collect::<Vec<_>>();
                formulas = copied_formula_inputs(
                    &current_names,
                    target_name,
                    &target,
                    &relocated,
                    target.start,
                    target_name,
                    true,
                )
                .map_err(|native| error(native.to_string()))?;
            }
        }
        let captured: BTreeMap<_, _> = payload
            .cells
            .iter()
            .map(|cell| (cell.address, &cell.fields))
            .collect();
        let mut changes = ChangeSet::new();
        for source in addresses(&payload.source_range) {
            let address = map_address(&payload.source_range, &target, source);
            let mut fields = captured
                .get(&source)
                .map(|fields| (*fields).clone())
                .unwrap_or_default();
            if let Some(formula) = formulas.get(&address) {
                fields.input = Some(formula.clone());
            }
            self.clipboard_replace(&mut changes, address, &fields);
        }
        for merge in &payload.merges {
            changes.push(Change::MergeCells {
                sheet: target.sheet,
                range: CellRange::new(
                    map_address(&payload.source_range, &target, merge.start),
                    map_address(&payload.source_range, &target, merge.end),
                )?,
            });
        }
        self.execute_clipboard_changes(changes, None)?;
        Ok(target)
    }

    /// TSV changes inputs only. Normal entries use the existing cell input parser;
    /// leading-zero/plus numbers and whitespace carry explicit literal intent.
    pub fn paste_tsv(&mut self, text: &str, selection: CellRange) -> ApiResult<CellRange> {
        let matrix = parse_tsv(text)?;
        let target =
            self.clipboard_destination(selection, matrix.len() as u32, matrix[0].len() as u32)?;
        self.clipboard_collision(&target)?;
        self.clipboard_workbook_preflight()?;
        let mut fields = BTreeMap::new();
        let mut formula_cells = Vec::new();
        for (row, record) in matrix.iter().enumerate() {
            for (column, text) in record.iter().enumerate() {
                let address = CellAddress::new(
                    target.sheet,
                    RowIndex(target.start.row.0 + row as u32),
                    ColumnIndex(target.start.column.0 + column as u32),
                );
                let input = external_input(text)?;
                if matches!(input, CellInput::Formula(_)) {
                    formula_cells.push(CellSnapshot {
                        address,
                        input: input.clone(),
                        format: None,
                    });
                }
                fields.insert(address, input);
            }
        }
        if !formula_cells.is_empty() {
            let names = self.clipboard_formula_names()?;
            let name = self.document.workbook.sheet_name(target.sheet).unwrap();
            let checked = copied_formula_inputs(
                &names,
                name,
                &target,
                &formula_cells,
                target.start,
                name,
                true,
            )
            .map_err(|native| error(native.to_string()))?;
            for (address, input) in checked {
                *fields.get_mut(&address).unwrap() = input;
            }
        }
        let mut changes = ChangeSet::new();
        for (address, input) in fields {
            let target_input = if input == CellInput::Empty
                && self.document.cell_format(address.sheet, address).is_none()
            {
                None
            } else {
                Some(&input)
            };
            if self.document.cell_input(address.sheet, address) != target_input {
                changes.push(Change::SetCellInput {
                    sheet: address.sheet,
                    address,
                    input,
                });
            }
        }
        self.execute_clipboard_changes(changes, None)?;
        Ok(target)
    }

    /// Cut capture remains read-only until this single bounded move commits.
    /// Cross-document/overlapping moves and unsupported native cases reject.
    pub fn cut_paste(
        &mut self,
        payload: &ClipboardPayload,
        selection: CellRange,
    ) -> ApiResult<CellRange> {
        if !self.cut_payload_valid(payload) {
            return Err(error("cut source is stale or belongs to another API"));
        }
        let target = self.clipboard_destination(selection, payload.rows(), payload.columns())?;
        if target == payload.source_range {
            return Ok(target);
        }
        if target.intersects(&payload.source_range) {
            return Err(error("overlapping cut ranges are unsupported"));
        }
        if area(&target)?.saturating_mul(2) > MAX_CLIPBOARD_CELLS {
            return Err(error(
                "cut source and destination exceed 10,000 affected cells",
            ));
        }
        self.clipboard_collision(&target)?;
        self.clipboard_workbook_preflight()?;
        // Measure the guaranteed source/destination inverse from borrowed old
        // fields before the native candidate snapshots either whole workbook.
        let mut preview = ChangeSet::new();
        let preview_fields: BTreeMap<_, _> = payload
            .cells
            .iter()
            .map(|cell| (cell.address, &cell.fields))
            .collect();
        for source in addresses(&payload.source_range) {
            let target_address = map_address(&payload.source_range, &target, source);
            let blank = CellFields::default();
            let fields = preview_fields.get(&source).copied().unwrap_or(&blank);
            self.clipboard_replace(&mut preview, target_address, fields);
            self.clipboard_replace(&mut preview, source, &blank);
        }
        for merge in &payload.merges {
            preview.push(Change::UnmergeCells {
                sheet: merge.sheet,
                range: merge.clone(),
            });
            preview.push(Change::MergeCells {
                sheet: target.sheet,
                range: CellRange::new(
                    map_address(&payload.source_range, &target, merge.start),
                    map_address(&payload.source_range, &target, merge.end),
                )?,
            });
        }
        crate::history::preflight_clipboard_history_budget(
            &self.document.workbook,
            &preview,
            self.history.limits(),
        )?;
        drop(preview);
        let before = self.document.snapshot();
        let formulas = if payload.source_range.sheet == target.sheet {
            self.native_same_sheet_cut_formulas(&before, &payload.source_range, &target)?
        } else {
            moved_formula_inputs(&before, &payload.source_range, target.start)
                .map_err(|native| error(native.to_string()))?
        };
        let captured: BTreeMap<_, _> = payload
            .cells
            .iter()
            .map(|cell| (cell.address, &cell.fields))
            .collect();
        let mut changes = ChangeSet::new();
        for merge in &payload.merges {
            changes.push(Change::UnmergeCells {
                sheet: merge.sheet,
                range: merge.clone(),
            });
        }
        let mut changed_addresses = BTreeSet::new();
        for source in addresses(&payload.source_range) {
            let address = map_address(&payload.source_range, &target, source);
            let mut fields = captured
                .get(&source)
                .map(|fields| (*fields).clone())
                .unwrap_or_default();
            if let Some(formula) = formulas.get(&address) {
                fields.input = Some(formula.clone());
            }
            self.clipboard_replace(&mut changes, address, &fields);
            self.clipboard_replace(&mut changes, source, &CellFields::default());
            changed_addresses.insert(address);
            changed_addresses.insert(source);
        }
        for (address, input) in formulas {
            if !target.contains(&address) {
                let fields = CellFields {
                    input: Some(input),
                    format: self.document.cell_format(address.sheet, address).cloned(),
                };
                self.clipboard_replace(&mut changes, address, &fields);
                changed_addresses.insert(address);
            }
        }
        if changed_addresses.len() > MAX_CLIPBOARD_CELLS {
            return Err(error(
                "cut including dependent formulas exceeds affected-cell bound",
            ));
        }
        for merge in &payload.merges {
            changes.push(Change::MergeCells {
                sheet: target.sheet,
                range: CellRange::new(
                    map_address(&payload.source_range, &target, merge.start),
                    map_address(&payload.source_range, &target, merge.end),
                )?,
            });
        }
        self.execute_clipboard_changes(changes, Some((payload.source_range.sheet, target.sheet)))?;
        Ok(target)
    }

    fn clipboard_replace(
        &self,
        changes: &mut ChangeSet,
        address: CellAddress,
        target: &CellFields,
    ) {
        let equal = self.document.cell_input(address.sheet, address) == target.input.as_ref()
            && self.document.cell_format(address.sheet, address) == target.format.as_ref();
        if !equal {
            replace_cell(changes, address, target);
        }
    }

    fn native_same_sheet_cut_formulas(
        &self,
        before: &crate::WorkbookSnapshot,
        source: &CellRange,
        target: &CellRange,
    ) -> ApiResult<BTreeMap<CellAddress, CellInput>> {
        let mut native = build_native_cut_model(before, source, target.start)
            .map_err(|native| error(native.to_string()))?;
        let captured = native.copy_to_clipboard().map_err(error)?;
        // This is the public serialized ClipboardData wire route used by the
        // engine bindings; no private fields or retained second undo stack.
        let mut bytes = NativeClipboardWire::default();
        serde_json::to_writer(&mut bytes, &captured).map_err(|native| error(native.to_string()))?;
        let mut wire: serde_json::Value =
            serde_json::from_slice(&bytes.0).map_err(|native| error(native.to_string()))?;
        let data = wire
            .as_object_mut()
            .and_then(|object| object.remove("data"))
            .ok_or_else(|| error("native clipboard data is unavailable"))?;
        let data = serde_json::from_value(data).map_err(|native| error(native.to_string()))?;
        drop(wire);
        drop(bytes);
        drop(captured);
        let source_index = before
            .sheets
            .iter()
            .position(|sheet| sheet.id == source.sheet)
            .expect("validated source");
        native
            .set_selected_cell(
                (target.start.row.0 + 1) as i32,
                (target.start.column.0 + 1) as i32,
            )
            .map_err(error)?;
        native
            .paste_from_clipboard(
                source_index as u32,
                (
                    (source.start.row.0 + 1) as i32,
                    (source.start.column.0 + 1) as i32,
                    (source.end.row.0 + 1) as i32,
                    (source.end.column.0 + 1) as i32,
                ),
                &data,
                true,
            )
            .map_err(error)?;
        extract_native_cut_formulas(&native, before, source, target.start)
            .map_err(|native| error(native.to_string()))
    }
}

fn external_input(text: &str) -> ApiResult<CellInput> {
    if let Some(literal) = text.strip_prefix('\'') {
        return Ok(CellInput::Text(literal.to_owned()));
    }
    if !text.trim_start().starts_with('=') && text.trim() != text {
        return Ok(CellInput::Text(text.to_owned()));
    }
    let parsed = EntryParser::parse(text)?.into_input()?;
    if matches!(parsed, CellInput::Number(_)) {
        let unsigned = text.trim().trim_start_matches('-');
        if text.trim().starts_with('+')
            || (unsigned.starts_with('0')
                && unsigned.as_bytes().get(1).is_some_and(u8::is_ascii_digit))
        {
            return Ok(CellInput::Text(text.to_owned()));
        }
    }
    Ok(parsed)
}
