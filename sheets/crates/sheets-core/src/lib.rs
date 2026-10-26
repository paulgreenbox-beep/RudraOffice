//! Canonical core spreadsheet types and immutable mutation workflow.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique sheet identifier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SheetId(pub u32);

impl fmt::Display for SheetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Zero-based row index.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RowIndex(pub u32);

impl fmt::Display for RowIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Zero-based column index.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ColumnIndex(pub u32);

impl fmt::Display for ColumnIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Revision counter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Revision(pub u64);

impl Revision {
    /// Bumps revision by exactly one, or returns an exhaustion error.
    pub fn next(self) -> Result<Self, CoreError> {
        self.0
            .checked_add(1)
            .map(Revision)
            .ok_or(CoreError::RevisionExhausted)
    }

    /// Alias for [`Revision::next`].
    pub fn checked_next(self) -> Result<Self, CoreError> {
        self.next()
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Cell address in a single-sheet coordinate system.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CellAddress {
    /// Sheet owner.
    pub sheet: SheetId,
    /// Zero-based row.
    pub row: RowIndex,
    /// Zero-based column.
    pub column: ColumnIndex,
}

impl CellAddress {
    pub const fn new(sheet: SheetId, row: RowIndex, column: ColumnIndex) -> Self {
        Self { sheet, row, column }
    }
}

/// Inclusive single-sheet cell range. The constructor normalizes order.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CellRange {
    /// Common sheet for the range. Mutating this field can make a range invalid; core validates it at every boundary.
    pub sheet: SheetId,
    /// Top-left address after normalization. Mutating this field can make a range invalid; core validates it at every boundary.
    pub start: CellAddress,
    /// Bottom-right address after normalization. Mutating this field can make a range invalid; core validates it at every boundary.
    pub end: CellAddress,
}

impl CellRange {
    /// Creates a normalized inclusive range.
    pub fn new(start: CellAddress, end: CellAddress) -> Result<Self, CoreError> {
        if start.sheet != end.sheet {
            return Err(CoreError::invalid_range("range must be on one sheet"));
        }
        let min_row = RowIndex(start.row.0.min(end.row.0));
        let max_row = RowIndex(start.row.0.max(end.row.0));
        let min_col = ColumnIndex(start.column.0.min(end.column.0));
        let max_col = ColumnIndex(start.column.0.max(end.column.0));
        Ok(Self {
            sheet: start.sheet,
            start: CellAddress::new(start.sheet, min_row, min_col),
            end: CellAddress::new(start.sheet, max_row, max_col),
        })
    }

    /// Sheet containing the range.
    pub const fn sheet(&self) -> SheetId {
        self.sheet
    }

    /// Top-left address after normalization.
    pub const fn start(&self) -> CellAddress {
        self.start
    }

    /// Bottom-right address after normalization.
    pub const fn end(&self) -> CellAddress {
        self.end
    }

    /// Validates the range's single-sheet and ordering invariants.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.start.sheet != self.sheet || self.end.sheet != self.sheet {
            return Err(CoreError::invalid_range(
                "range addresses must use range sheet",
            ));
        }
        if self.start.row.0 > self.end.row.0 || self.start.column.0 > self.end.column.0 {
            return Err(CoreError::invalid_range("range bounds are not normalized"));
        }
        Ok(())
    }

    pub fn contains(&self, address: &CellAddress) -> bool {
        self.validate().is_ok()
            && address.sheet == self.sheet
            && address.row.0 >= self.start.row.0
            && address.row.0 <= self.end.row.0
            && address.column.0 >= self.start.column.0
            && address.column.0 <= self.end.column.0
    }

    pub fn intersects(&self, other: &CellRange) -> bool {
        self.validate().is_ok()
            && other.validate().is_ok()
            && self.sheet == other.sheet
            && self.start.row.0 <= other.end.row.0
            && self.end.row.0 >= other.start.row.0
            && self.start.column.0 <= other.end.column.0
            && self.end.column.0 >= other.start.column.0
    }

    /// Returns inclusive row cardinality as `u64`; malformed public-field ranges return zero.
    pub fn rows(&self) -> u64 {
        if self.validate().is_err() {
            return 0;
        }
        self.end.row.0 as u64 - self.start.row.0 as u64 + 1
    }

    /// Returns inclusive column cardinality as `u64`; malformed public-field ranges return zero.
    pub fn cols(&self) -> u64 {
        if self.validate().is_err() {
            return 0;
        }
        self.end.column.0 as u64 - self.start.column.0 as u64 + 1
    }
}

/// Canonical spreadsheet error value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CellError {
    Div0,
    Value,
    Ref,
    Name,
    NA,
    Circular,
}

impl CellError {
    pub const fn token(&self) -> &'static str {
        match self {
            Self::Div0 => "#DIV/0!",
            Self::Value => "#VALUE!",
            Self::Ref => "#REF!",
            Self::Name => "#NAME?",
            Self::NA => "#N/A",
            Self::Circular => "#CIRCULAR!",
        }
    }
}

impl fmt::Display for CellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.token())
    }
}

/// Raw user-facing cell entry text/number/boolean/value types.
#[derive(Clone, Debug, PartialEq)]
pub enum CellInput {
    /// Empty input.
    Empty,
    /// Plain text.
    Text(String),
    /// Numeric value.
    Number(f64),
    /// Percent value (stored as decimal fraction).
    Percent(f64),
    /// Date serial serial (engine format independent).
    Date(f64),
    /// Time serial (engine format independent).
    Time(f64),
    /// Boolean.
    Boolean(bool),
    /// Normalized formula without leading `=`.
    Formula(String),
}

/// Maximum normalized formula size accepted by the core model.
pub const MAX_FORMULA_BYTES: usize = 16 * 1024;
impl CellInput {
    pub fn formula(raw: impl Into<String>) -> Result<Self, CoreError> {
        normalize_formula(raw.into()).map(Self::Formula)
    }

    pub fn is_formula(&self) -> bool {
        matches!(self, Self::Formula(_))
    }

    pub fn as_formula(&self) -> Option<&str> {
        if let Self::Formula(formula) = self {
            Some(formula.as_str())
        } else {
            None
        }
    }
}

fn normalize_formula(raw: String) -> Result<String, CoreError> {
    let normalized = raw.trim();
    let normalized = normalized.strip_prefix('=').unwrap_or(normalized).trim();
    if normalized.is_empty() {
        return Err(CoreError::InvalidFormula("formula cannot be empty".into()));
    }
    if normalized.len() > MAX_FORMULA_BYTES {
        return Err(CoreError::InvalidFormula(format!(
            "formula exceeds {MAX_FORMULA_BYTES} byte limit"
        )));
    }
    Ok(normalized.to_string())
}

fn ensure_finite_f64(value: f64, kind: &'static str) -> Result<f64, CoreError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(CoreError::NonFiniteNumericValue(kind.into()))
    }
}

/// Calculated, non-input values stored after engine pass.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Empty,
    Text(String),
    Number(f64),
    Percent(f64),
    Date(f64),
    Time(f64),
    Boolean(bool),
    Error(CellError),
}

impl CellValue {
    pub fn number(value: f64) -> Result<Self, CoreError> {
        ensure_finite_f64(value, "Number").map(Self::Number)
    }

    pub fn percent(value: f64) -> Result<Self, CoreError> {
        ensure_finite_f64(value, "Percent").map(Self::Percent)
    }

    pub fn date(value: f64) -> Result<Self, CoreError> {
        ensure_finite_f64(value, "Date").map(Self::Date)
    }

    pub fn time(value: f64) -> Result<Self, CoreError> {
        ensure_finite_f64(value, "Time").map(Self::Time)
    }
}

/// Simple color type.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// Font description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Font {
    pub family: String,
    pub size: u16,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: "Arial".into(),
            size: 11,
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BorderStyle {
    None,
    Solid,
    Dashed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Border {
    pub style: BorderStyle,
    pub color: Option<Color>,
}

impl Default for Border {
    fn default() -> Self {
        Self {
            style: BorderStyle::None,
            color: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Borders {
    pub top: Option<Border>,
    pub right: Option<Border>,
    pub bottom: Option<Border>,
    pub left: Option<Border>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HorizontalAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerticalAlign {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Alignment {
    pub horizontal: Option<HorizontalAlign>,
    pub vertical: Option<VerticalAlign>,
    pub wrap: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum NumberFormat {
    #[default]
    General,
    Decimal(u8),
    Percent(u8),
    Currency {
        symbol: String,
        precision: u8,
    },
    Date,
    Time,
    Custom(String),
}

/// Engine-agnostic formatting block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellFormat {
    pub font: Font,
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub borders: Borders,
    pub alignment: Alignment,
    pub number_format: NumberFormat,
}

impl Default for CellFormat {
    fn default() -> Self {
        Self {
            font: Font::default(),
            foreground: None,
            background: None,
            borders: Borders::default(),
            alignment: Alignment::default(),
            number_format: NumberFormat::General,
        }
    }
}

/// Calculated cell in engine response.
#[derive(Clone, Debug, PartialEq)]
pub struct CalculatedCell {
    pub address: CellAddress,
    pub value: CellValue,
}

/// Per-cell persisted input snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct CellSnapshot {
    pub address: CellAddress,
    pub input: CellInput,
    pub format: Option<CellFormat>,
}

/// One predicate used to filter a column in an auto-filter range.
#[derive(Clone, Debug, PartialEq)]
pub enum FilterCriterion {
    Values(Vec<String>),
    Equals(String),
    NotEquals(String),
    Contains(String),
    Blank,
    NonBlank,
    NumberEquals(f64),
    GreaterThan(f64),
    LessThan(f64),
}

/// Persisted filter range and its per-column criteria.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoFilter {
    pub range: CellRange,
    pub has_header: bool,
    pub criteria: BTreeMap<ColumnIndex, FilterCriterion>,
}

impl AutoFilter {
    /// Checks range normalization, criterion columns, and numeric criterion values.
    pub fn validate(&self) -> Result<(), CoreError> {
        self.range.validate()?;
        for (column, criterion) in &self.criteria {
            if column.0 < self.range.start.column.0 || column.0 > self.range.end.column.0 {
                return Err(CoreError::InvalidRange(format!(
                    "auto-filter criterion column {column} is outside the filter range"
                )));
            }
            let value = match criterion {
                FilterCriterion::NumberEquals(value)
                | FilterCriterion::GreaterThan(value)
                | FilterCriterion::LessThan(value) => Some(*value),
                FilterCriterion::Values(_)
                | FilterCriterion::Equals(_)
                | FilterCriterion::NotEquals(_)
                | FilterCriterion::Contains(_)
                | FilterCriterion::Blank
                | FilterCriterion::NonBlank => None,
            };
            if value.is_some_and(|value| !value.is_finite()) {
                return Err(CoreError::NonFiniteNumericValue(
                    "auto-filter criterion".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Per-sheet persisted snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetSnapshot {
    pub id: SheetId,
    pub name: String,
    pub cells: Vec<CellSnapshot>,
    pub merges: Vec<CellRange>,
    pub row_dimensions: Vec<RowDimension>,
    pub column_dimensions: Vec<ColumnDimension>,
    pub auto_filter: Option<AutoFilter>,
    pub manual_hidden_rows: Vec<RowIndex>,
    pub manual_hidden_columns: Vec<ColumnIndex>,
}

/// Workbook snapshot used by engine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkbookSnapshot {
    pub revision: Revision,
    pub active_sheet: SheetId,
    pub sheets: Vec<SheetSnapshot>,
}

/// Default grid dimensions in unzoomed engine pixels.
///
/// The production adapter supplies these values from the engine; UI code must
/// not infer them from Excel's character-width or point units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DefaultGridMetrics {
    pub row_height: f64,
    pub column_width: f64,
}

/// Row dimension in unzoomed engine pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct RowDimension {
    pub row: RowIndex,
    pub height: f64,
}

/// Column dimension in unzoomed engine pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnDimension {
    pub column: ColumnIndex,
    pub width: f64,
}

/// Mutation set fed to [Workbook::prepare].
#[derive(Clone, Debug, Default)]
pub struct ChangeSet {
    pub changes: Vec<Change>,
}

impl ChangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, change: Change) -> Self {
        self.changes.push(change);
        self
    }

    pub fn push(&mut self, change: Change) {
        self.changes.push(change);
    }
}

#[derive(Clone, Debug)]
pub enum Change {
    AddSheet {
        name: Option<String>,
    },
    DeleteSheet {
        sheet: SheetId,
    },
    RenameSheet {
        sheet: SheetId,
        name: String,
    },
    MoveSheet {
        sheet: SheetId,
        target_index: usize,
    },
    DuplicateSheet {
        sheet: SheetId,
        name: Option<String>,
    },
    SetCellInput {
        sheet: SheetId,
        address: CellAddress,
        input: CellInput,
    },
    ClearCell {
        sheet: SheetId,
        address: CellAddress,
    },
    SetCellFormat {
        sheet: SheetId,
        address: CellAddress,
        format: CellFormat,
    },
    ClearCellFormat {
        sheet: SheetId,
        address: CellAddress,
    },
    MergeCells {
        sheet: SheetId,
        range: CellRange,
    },
    UnmergeCells {
        sheet: SheetId,
        range: CellRange,
    },
    SetRowDimension {
        sheet: SheetId,
        row: RowIndex,
        height: f64,
    },
    ClearRowDimension {
        sheet: SheetId,
        row: RowIndex,
    },
    SetColumnDimension {
        sheet: SheetId,
        column: ColumnIndex,
        width: f64,
    },
    ClearColumnDimension {
        sheet: SheetId,
        column: ColumnIndex,
    },
    SetActiveSheet {
        sheet: SheetId,
    },
    SetAutoFilter {
        sheet: SheetId,
        filter: Option<AutoFilter>,
    },
    SetManualHiddenRows {
        sheet: SheetId,
        rows: Vec<RowIndex>,
    },
    SetManualHiddenColumns {
        sheet: SheetId,
        columns: Vec<ColumnIndex>,
    },
    InsertRows {
        sheet: SheetId,
        at: RowIndex,
        count: u32,
    },
    DeleteRows {
        sheet: SheetId,
        at: RowIndex,
        count: u32,
    },
    InsertColumns {
        sheet: SheetId,
        at: ColumnIndex,
        count: u32,
    },
    DeleteColumns {
        sheet: SheetId,
        at: ColumnIndex,
        count: u32,
    },
}

/// Engine request with canonical snapshot + base revision.
#[derive(Clone, Debug)]
pub struct EngineRequest {
    base_revision: Revision,
    workbook: WorkbookSnapshot,
}

impl EngineRequest {
    pub fn base_revision(&self) -> Revision {
        self.base_revision
    }

    pub fn workbook(&self) -> &WorkbookSnapshot {
        &self.workbook
    }
}

/// Engine response containing calculated values only.
#[derive(Clone, Debug)]
pub struct EngineResponse {
    base_revision: Revision,
    calculated_cells: Vec<CalculatedCell>,
}

impl EngineResponse {
    pub fn new(base_revision: Revision, calculated_cells: Vec<CalculatedCell>) -> Self {
        Self {
            base_revision,
            calculated_cells,
        }
    }

    pub fn base_revision(&self) -> Revision {
        self.base_revision
    }

    pub fn calculated_cells(&self) -> &[CalculatedCell] {
        &self.calculated_cells
    }
}

/// Engine contract to be implemented by adapters.
pub trait SpreadsheetEngine {
    type Error: Error;

    fn evaluate(&mut self, request: EngineRequest) -> Result<EngineResponse, Self::Error>;
}

static NEXT_WORKBOOK_TOKEN: AtomicU64 = AtomicU64::new(1);

/// Read-only access to prepared plan request.
pub struct MutationPlan {
    base_revision: Revision,
    workbook_token: u64,
    request: EngineRequest,
    candidate: Workbook,
    committed: bool,
}

impl MutationPlan {
    pub fn engine_request(&self) -> EngineRequest {
        self.request.clone()
    }

    pub fn request(&self) -> &EngineRequest {
        &self.request
    }

    pub fn base_revision(&self) -> Revision {
        self.base_revision
    }
}

/// Error type for validation and contract breaches.
#[derive(Debug)]
pub enum CoreError {
    /// Provided sheet name is invalid.
    InvalidSheetName(String),
    /// Rejected malformed range.
    InvalidRange(String),
    /// Snapshot cannot be restored as a coherent workbook.
    InvalidSnapshot(String),
    /// Formula text is invalid.
    InvalidFormula(String),
    /// Input workbook revision is stale or mismatched.
    RevisionMismatch { expected: Revision, got: Revision },
    /// Duplicate sheet name conflict.
    DuplicateSheetName(String),
    /// Referenced sheet does not exist.
    SheetNotFound(SheetId),
    /// Cannot delete the last remaining sheet.
    CannotDeleteFinalSheet,
    /// A provided address is not present in the target snapshot.
    InvalidAddress(CellAddress),
    /// Engine response tried to change non-formula cell.
    ResponseForNonFormula(CellAddress),
    /// Formula evaluation response was structurally invalid.
    InvalidEngineResponse(String),
    /// Formula output used non-finite numeric value.
    NonFiniteNumericValue(String),
    /// Plan commit attempted after previous successful commit.
    PlanAlreadyCommitted,
    /// Plan belongs to another workbook instance.
    WorkbookMismatch,
    /// Revision cannot be incremented further.
    RevisionExhausted,
    /// Feature exists in spec but is not implemented here.
    Unsupported(String),
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSheetName(name) => write!(f, "invalid sheet name: {name}"),
            Self::InvalidRange(msg) => write!(f, "invalid range: {msg}"),
            Self::InvalidSnapshot(msg) => write!(f, "invalid workbook snapshot: {msg}"),
            Self::InvalidFormula(msg) => write!(f, "invalid formula: {msg}"),
            Self::RevisionMismatch { expected, got } => {
                write!(f, "revision mismatch: expected {expected}, got {got}")
            }
            Self::DuplicateSheetName(name) => write!(f, "duplicate sheet name: {name}"),
            Self::SheetNotFound(id) => write!(f, "sheet not found: {id}"),
            Self::CannotDeleteFinalSheet => write!(f, "cannot delete the final sheet"),
            Self::InvalidAddress(addr) => write!(
                f,
                "invalid address: sheet={}, row={}, column={}",
                addr.sheet, addr.row, addr.column
            ),
            Self::ResponseForNonFormula(addr) => write!(
                f,
                "engine response for non-formula address: sheet={}, row={}, column={}",
                addr.sheet, addr.row, addr.column
            ),
            Self::InvalidEngineResponse(msg) => write!(f, "invalid engine response: {msg}"),
            Self::NonFiniteNumericValue(kind) => write!(f, "non-finite numeric value: {kind}"),
            Self::PlanAlreadyCommitted => write!(f, "mutation plan was already committed"),
            Self::WorkbookMismatch => write!(f, "mutation plan belongs to another workbook"),
            Self::RevisionExhausted => write!(f, "workbook revision exhausted"),
            Self::Unsupported(feature) => write!(f, "unsupported feature: {feature}"),
        }
    }
}

impl Error for CoreError {}

impl CoreError {
    fn invalid_sheet_name(msg: impl Into<String>) -> Self {
        Self::InvalidSheetName(msg.into())
    }

    fn invalid_range(msg: impl Into<String>) -> Self {
        Self::InvalidRange(msg.into())
    }
}

#[derive(Clone, Default)]
struct Cell {
    input: Option<CellInput>,
    format: Option<CellFormat>,
    calculated: Option<CellValue>,
}

#[derive(Clone)]
struct Sheet {
    id: SheetId,
    name: String,
    cells: BTreeMap<(RowIndex, ColumnIndex), Cell>,
    merges: BTreeSet<CellRange>,
    row_dimensions: BTreeMap<RowIndex, f64>,
    column_dimensions: BTreeMap<ColumnIndex, f64>,
    auto_filter: Option<AutoFilter>,
    manual_hidden_rows: BTreeSet<RowIndex>,
    manual_hidden_columns: BTreeSet<ColumnIndex>,
}

impl Sheet {
    fn new(id: SheetId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            cells: BTreeMap::new(),
            merges: BTreeSet::new(),
            row_dimensions: BTreeMap::new(),
            column_dimensions: BTreeMap::new(),
            auto_filter: None,
            manual_hidden_rows: BTreeSet::new(),
            manual_hidden_columns: BTreeSet::new(),
        }
    }

    fn to_snapshot(&self) -> SheetSnapshot {
        let cells = self
            .cells
            .iter()
            .filter_map(|((row, col), cell)| {
                if cell.input.is_none() && cell.format.is_none() && cell.calculated.is_none() {
                    return None;
                }
                Some(CellSnapshot {
                    address: CellAddress::new(self.id, *row, *col),
                    input: cell.input.clone().unwrap_or(CellInput::Empty),
                    format: cell.format.clone(),
                })
            })
            .collect();

        let merges = self.merges.iter().cloned().collect();
        let row_dimensions = self
            .row_dimensions
            .iter()
            .map(|(row, height)| RowDimension {
                row: *row,
                height: *height,
            })
            .collect();
        let column_dimensions = self
            .column_dimensions
            .iter()
            .map(|(column, width)| ColumnDimension {
                column: *column,
                width: *width,
            })
            .collect();

        SheetSnapshot {
            id: self.id,
            name: self.name.clone(),
            cells,
            merges,
            row_dimensions,
            column_dimensions,
            auto_filter: self.auto_filter.clone(),
            manual_hidden_rows: self.manual_hidden_rows.iter().copied().collect(),
            manual_hidden_columns: self.manual_hidden_columns.iter().copied().collect(),
        }
    }

    fn from_snapshot(snapshot: &SheetSnapshot) -> Self {
        let mut cells = BTreeMap::new();
        for cell in &snapshot.cells {
            cells.insert(
                (cell.address.row, cell.address.column),
                Cell {
                    input: Some(cell.input.clone()),
                    format: cell.format.clone(),
                    calculated: None,
                },
            );
        }
        Self {
            id: snapshot.id,
            name: snapshot.name.clone(),
            cells,
            merges: snapshot.merges.iter().cloned().collect(),
            row_dimensions: snapshot
                .row_dimensions
                .iter()
                .map(|r| (r.row, r.height))
                .collect(),
            column_dimensions: snapshot
                .column_dimensions
                .iter()
                .map(|c| (c.column, c.width))
                .collect(),
            auto_filter: snapshot.auto_filter.clone(),
            manual_hidden_rows: snapshot.manual_hidden_rows.iter().copied().collect(),
            manual_hidden_columns: snapshot.manual_hidden_columns.iter().copied().collect(),
        }
    }
}

fn shift_rows(sheet: &mut Sheet, at: u32, count: u32, delete: bool) -> Result<(), CoreError> {
    let end = (at as u64) + (count as u64);
    let mut cells = BTreeMap::new();
    for ((row, column), cell) in std::mem::take(&mut sheet.cells) {
        let row = row.0 as u64;
        if delete && row >= at as u64 && row < end {
            continue;
        }
        let new_row = if delete {
            if row >= end { row - count as u64 } else { row }
        } else if row >= at as u64 {
            row + count as u64
        } else {
            row
        };
        if new_row > u32::MAX as u64 {
            return Err(CoreError::InvalidRange(
                "row shift overflows index space".into(),
            ));
        }
        cells.insert((RowIndex(new_row as u32), column), cell);
    }
    sheet.cells = cells;
    let mut dimensions = BTreeMap::new();
    for (row, height) in std::mem::take(&mut sheet.row_dimensions) {
        let row = row.0 as u64;
        if delete && row >= at as u64 && row < end {
            continue;
        }
        let new_row = if delete {
            if row >= end { row - count as u64 } else { row }
        } else if row >= at as u64 {
            row + count as u64
        } else {
            row
        };
        if new_row <= u32::MAX as u64 {
            dimensions.insert(RowIndex(new_row as u32), height);
        }
    }
    sheet.row_dimensions = dimensions;
    sheet.merges = sheet
        .merges
        .iter()
        .filter_map(|range| shift_range_rows(range, at, count, delete))
        .collect();
    Ok(())
}

fn shift_columns(sheet: &mut Sheet, at: u32, count: u32, delete: bool) -> Result<(), CoreError> {
    let end = (at as u64) + (count as u64);
    let mut cells = BTreeMap::new();
    for ((row, column), cell) in std::mem::take(&mut sheet.cells) {
        let column = column.0 as u64;
        if delete && column >= at as u64 && column < end {
            continue;
        }
        let new_column = if delete {
            if column >= end {
                column - count as u64
            } else {
                column
            }
        } else if column >= at as u64 {
            column + count as u64
        } else {
            column
        };
        if new_column > u32::MAX as u64 {
            return Err(CoreError::InvalidRange(
                "column shift overflows index space".into(),
            ));
        }
        cells.insert((row, ColumnIndex(new_column as u32)), cell);
    }
    sheet.cells = cells;
    let mut dimensions = BTreeMap::new();
    for (column, width) in std::mem::take(&mut sheet.column_dimensions) {
        let column = column.0 as u64;
        if delete && column >= at as u64 && column < end {
            continue;
        }
        let new_column = if delete {
            if column >= end {
                column - count as u64
            } else {
                column
            }
        } else if column >= at as u64 {
            column + count as u64
        } else {
            column
        };
        if new_column <= u32::MAX as u64 {
            dimensions.insert(ColumnIndex(new_column as u32), width);
        }
    }
    sheet.column_dimensions = dimensions;
    sheet.merges = sheet
        .merges
        .iter()
        .filter_map(|range| shift_range_columns(range, at, count, delete))
        .collect();
    Ok(())
}

fn shift_range_rows(range: &CellRange, at: u32, count: u32, delete: bool) -> Option<CellRange> {
    let start = range.start.row.0 as u64;
    let end = range.end.row.0 as u64;
    let cut_end = at as u64 + count as u64;
    if delete && start >= at as u64 && start < cut_end && end < cut_end {
        return None;
    }
    let map = |value: u64| -> Option<u32> {
        let mapped = if delete {
            if value >= cut_end {
                value - count as u64
            } else {
                value
            }
        } else if value >= at as u64 {
            value + count as u64
        } else {
            value
        };
        (mapped <= u32::MAX as u64).then_some(mapped as u32)
    };
    let new_start = map(start)?;
    let new_end = map(end)?;
    CellRange::new(
        CellAddress::new(range.sheet, RowIndex(new_start), range.start.column),
        CellAddress::new(range.sheet, RowIndex(new_end), range.end.column),
    )
    .ok()
}

fn shift_range_columns(range: &CellRange, at: u32, count: u32, delete: bool) -> Option<CellRange> {
    let start = range.start.column.0 as u64;
    let end = range.end.column.0 as u64;
    let cut_end = at as u64 + count as u64;
    if delete && start >= at as u64 && start < cut_end && end < cut_end {
        return None;
    }
    let map = |value: u64| -> Option<u32> {
        let mapped = if delete {
            if value >= cut_end {
                value - count as u64
            } else {
                value
            }
        } else if value >= at as u64 {
            value + count as u64
        } else {
            value
        };
        (mapped <= u32::MAX as u64).then_some(mapped as u32)
    };
    let new_start = map(start)?;
    let new_end = map(end)?;
    CellRange::new(
        CellAddress::new(range.sheet, range.start.row, ColumnIndex(new_start)),
        CellAddress::new(range.sheet, range.end.row, ColumnIndex(new_end)),
    )
    .ok()
}

/// Workbook owning all canonical cross-crate spreadsheet types.
#[derive(Clone)]
pub struct Workbook {
    revision: Revision,
    active_sheet: SheetId,
    sheets: Vec<Sheet>,
    workbook_token: u64,
}

/// Bounded view of memory and record counts owned by a workbook.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkbookResourceUsage {
    /// Number of sheets.
    pub sheets: usize,
    /// Stored cells and sheet metadata records.
    pub records: usize,
    /// Formula inputs.
    pub formulas: usize,
    /// UTF-8 byte lengths of owned workbook text.
    pub owned_text_bytes: usize,
    /// Conservative estimate of owned workbook bytes, including collection overhead.
    pub estimated_owned_bytes: usize,
}

const BTREE_ENTRY_OVERHEAD_WORDS: usize = 4;
const BTREE_NODE_OVERHEAD_WORDS: usize = 16;

fn estimated_vec_bytes<T>(capacity: usize) -> usize {
    capacity.saturating_mul(std::mem::size_of::<T>())
}

fn estimated_btree_map_bytes<K, V>(entries: usize) -> usize {
    // Account for spare node slots as well as stored entries. Three slots per
    // entry cover partially occupied non-root nodes; the first node has eleven.
    let slot_bytes = std::mem::size_of::<(K, V)>();
    let per_entry = slot_bytes
        .saturating_mul(3)
        .saturating_add(BTREE_ENTRY_OVERHEAD_WORDS.saturating_mul(std::mem::size_of::<usize>()));
    entries
        .saturating_mul(per_entry)
        .saturating_add(if entries == 0 {
            0
        } else {
            slot_bytes.saturating_mul(11).saturating_add(
                BTREE_NODE_OVERHEAD_WORDS.saturating_mul(std::mem::size_of::<usize>()),
            )
        })
}

fn estimated_btree_set_bytes<K>(entries: usize) -> usize {
    let slot_bytes = std::mem::size_of::<K>();
    let per_entry = slot_bytes
        .saturating_mul(3)
        .saturating_add(BTREE_ENTRY_OVERHEAD_WORDS.saturating_mul(std::mem::size_of::<usize>()));
    entries
        .saturating_mul(per_entry)
        .saturating_add(if entries == 0 {
            0
        } else {
            slot_bytes.saturating_mul(11).saturating_add(
                BTREE_NODE_OVERHEAD_WORDS.saturating_mul(std::mem::size_of::<usize>()),
            )
        })
}

fn account_owned_string(
    usage: &mut WorkbookResourceUsage,
    estimated_bytes: &mut usize,
    value: &String,
) {
    usage.owned_text_bytes = usage.owned_text_bytes.saturating_add(value.len());
    *estimated_bytes = estimated_bytes.saturating_add(value.capacity().max(value.len()));
}

impl Default for Workbook {
    fn default() -> Self {
        Self::new()
    }
}

impl Workbook {
    /// Creates a workbook with one default sheet.
    pub fn new() -> Self {
        let sheet_id = SheetId(1);
        let sheet = Sheet::new(sheet_id, "Sheet1");
        Self {
            revision: Revision(0),
            active_sheet: sheet_id,
            sheets: vec![sheet],
            workbook_token: NEXT_WORKBOOK_TOKEN.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Restores a coherent workbook snapshot while assigning a fresh internal token.
    ///
    /// Calculated values are intentionally not part of a persisted snapshot; callers
    /// should run the formula engine after restoration before exposing the workbook.
    pub fn from_snapshot(mut snapshot: WorkbookSnapshot) -> Result<Self, CoreError> {
        if snapshot.sheets.is_empty() {
            return Err(CoreError::InvalidSnapshot(
                "workbook must contain at least one sheet".into(),
            ));
        }

        let mut sheet_ids = BTreeSet::new();
        let mut sheet_names = BTreeSet::new();
        for sheet in &mut snapshot.sheets {
            if sheet.id.0 == 0 {
                return Err(CoreError::InvalidSnapshot(
                    "sheet identifier must be non-zero".into(),
                ));
            }
            if !sheet_ids.insert(sheet.id) {
                return Err(CoreError::InvalidSnapshot(format!(
                    "duplicate sheet identifier: {}",
                    sheet.id
                )));
            }
            validate_sheet_name(&sheet.name)?;
            if !sheet_names.insert(sheet.name.to_ascii_lowercase()) {
                return Err(CoreError::DuplicateSheetName(sheet.name.clone()));
            }

            let mut addresses = BTreeSet::new();
            for cell in &mut sheet.cells {
                if cell.address.sheet != sheet.id {
                    return Err(CoreError::InvalidAddress(cell.address));
                }
                if !addresses.insert((cell.address.row, cell.address.column)) {
                    return Err(CoreError::InvalidSnapshot(format!(
                        "duplicate cell address on sheet {}: row={}, column={}",
                        sheet.id, cell.address.row, cell.address.column
                    )));
                }
                cell.input = Self::ensure_formula_input(&cell.input)?;
            }

            let mut merges = BTreeSet::new();
            for range in &sheet.merges {
                Self::validate_range(range)?;
                if range.sheet != sheet.id {
                    return Err(CoreError::InvalidRange(format!(
                        "merge belongs to sheet {}, not {}",
                        range.sheet, sheet.id
                    )));
                }
                if !merges.insert(range.clone()) {
                    return Err(CoreError::InvalidSnapshot(format!(
                        "duplicate merge on sheet {}",
                        sheet.id
                    )));
                }
            }

            let mut row_dimensions = BTreeSet::new();
            for dimension in &sheet.row_dimensions {
                if dimension.row.0 == u32::MAX
                    || !dimension.height.is_finite()
                    || dimension.height <= 0.0
                {
                    return Err(CoreError::InvalidSnapshot(
                        "row dimensions must have a valid index and positive height".into(),
                    ));
                }
                if !row_dimensions.insert(dimension.row) {
                    return Err(CoreError::InvalidSnapshot(format!(
                        "duplicate row dimension on sheet {}: {}",
                        sheet.id, dimension.row
                    )));
                }
            }

            let mut column_dimensions = BTreeSet::new();
            for dimension in &sheet.column_dimensions {
                if !dimension.width.is_finite() || dimension.width <= 0.0 {
                    return Err(CoreError::InvalidSnapshot(
                        "column dimensions must have positive finite width".into(),
                    ));
                }
                if !column_dimensions.insert(dimension.column) {
                    return Err(CoreError::InvalidSnapshot(format!(
                        "duplicate column dimension on sheet {}: {}",
                        sheet.id, dimension.column
                    )));
                }
            }

            if let Some(filter) = &sheet.auto_filter {
                filter.validate()?;
                if filter.range.sheet != sheet.id {
                    return Err(CoreError::InvalidRange(format!(
                        "auto-filter range belongs to sheet {}, not {}",
                        filter.range.sheet, sheet.id
                    )));
                }
            }

            let mut manual_hidden_rows = BTreeSet::new();
            for row in &sheet.manual_hidden_rows {
                if row.0 == u32::MAX {
                    return Err(CoreError::InvalidSnapshot(
                        "manual hidden row index must not be u32::MAX".into(),
                    ));
                }
                if !manual_hidden_rows.insert(*row) {
                    return Err(CoreError::InvalidSnapshot(format!(
                        "duplicate manual hidden row on sheet {}: {}",
                        sheet.id, row
                    )));
                }
            }

            let mut manual_hidden_columns = BTreeSet::new();
            for column in &sheet.manual_hidden_columns {
                if column.0 == u32::MAX {
                    return Err(CoreError::InvalidSnapshot(
                        "manual hidden column index must not be u32::MAX".into(),
                    ));
                }
                if !manual_hidden_columns.insert(*column) {
                    return Err(CoreError::InvalidSnapshot(format!(
                        "duplicate manual hidden column on sheet {}: {}",
                        sheet.id, column
                    )));
                }
            }
        }

        if !sheet_ids.contains(&snapshot.active_sheet) {
            return Err(CoreError::SheetNotFound(snapshot.active_sheet));
        }

        Ok(Self {
            revision: snapshot.revision,
            active_sheet: snapshot.active_sheet,
            sheets: snapshot.sheets.iter().map(Sheet::from_snapshot).collect(),
            workbook_token: NEXT_WORKBOOK_TOKEN.fetch_add(1, Ordering::Relaxed),
        })
    }

    /// Current workbook revision.
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Returns current active sheet id.
    pub fn active_sheet(&self) -> SheetId {
        self.active_sheet
    }

    /// Total sheet count.
    pub fn sheet_count(&self) -> usize {
        self.sheets.len()
    }

    /// Iterates sheet identifiers and names in workbook order without allocating.
    pub fn sheet_names(&self) -> impl ExactSizeIterator<Item = (SheetId, &str)> + '_ {
        self.sheets
            .iter()
            .map(|sheet| (sheet.id, sheet.name.as_str()))
    }

    /// Returns sheet name by id.
    pub fn sheet_name(&self, sheet: SheetId) -> Option<&str> {
        self.sheet_by_id(sheet).map(|s| s.name.as_str())
    }

    /// Returns borrowed merge ranges for a sheet in canonical order.
    pub fn sheet_merges(&self, sheet: SheetId) -> Option<&BTreeSet<CellRange>> {
        self.sheet_by_id(sheet).map(|s| &s.merges)
    }

    /// Returns the persisted auto-filter for a sheet.
    pub fn auto_filter(&self, sheet: SheetId) -> Option<&AutoFilter> {
        self.sheet_by_id(sheet).and_then(|s| s.auto_filter.as_ref())
    }

    /// Returns canonical manually hidden row indexes for a sheet.
    pub fn manual_hidden_rows(&self, sheet: SheetId) -> Option<&BTreeSet<RowIndex>> {
        self.sheet_by_id(sheet).map(|s| &s.manual_hidden_rows)
    }

    /// Returns canonical manually hidden column indexes for a sheet.
    pub fn manual_hidden_columns(&self, sheet: SheetId) -> Option<&BTreeSet<ColumnIndex>> {
        self.sheet_by_id(sheet).map(|s| &s.manual_hidden_columns)
    }

    /// Estimates workbook clone resources by scanning the owned state without cloning it.
    ///
    /// The byte estimate includes inline workbook/sheet/cell storage, vector capacities,
    /// B-tree entries with a conservative node allowance, and capacities for owned text.
    /// Saturation to `usize::MAX` signals that the estimate overflowed.
    pub fn resource_usage(&self) -> WorkbookResourceUsage {
        let mut usage = WorkbookResourceUsage {
            sheets: self.sheets.len(),
            ..WorkbookResourceUsage::default()
        };
        let mut estimated_bytes = std::mem::size_of::<Workbook>()
            .saturating_add(estimated_vec_bytes::<Sheet>(self.sheets.capacity()));

        for sheet in &self.sheets {
            account_owned_string(&mut usage, &mut estimated_bytes, &sheet.name);

            let cell_count = sheet.cells.len();
            usage.records = usage.records.saturating_add(cell_count);
            estimated_bytes = estimated_bytes
                .saturating_add(estimated_btree_map_bytes::<(RowIndex, ColumnIndex), Cell>(
                    cell_count,
                ));
            for cell in sheet.cells.values() {
                if let Some(input) = &cell.input {
                    match input {
                        CellInput::Text(value) => {
                            account_owned_string(&mut usage, &mut estimated_bytes, value);
                        }
                        CellInput::Formula(value) => {
                            usage.formulas = usage.formulas.saturating_add(1);
                            account_owned_string(&mut usage, &mut estimated_bytes, value);
                        }
                        CellInput::Empty
                        | CellInput::Number(_)
                        | CellInput::Percent(_)
                        | CellInput::Date(_)
                        | CellInput::Time(_)
                        | CellInput::Boolean(_) => {}
                    }
                }

                if let Some(format) = &cell.format {
                    estimated_bytes =
                        estimated_bytes.saturating_add(std::mem::size_of::<CellFormat>());
                    account_owned_string(&mut usage, &mut estimated_bytes, &format.font.family);
                    match &format.number_format {
                        NumberFormat::Currency { symbol, .. } => {
                            account_owned_string(&mut usage, &mut estimated_bytes, symbol);
                        }
                        NumberFormat::Custom(pattern) => {
                            account_owned_string(&mut usage, &mut estimated_bytes, pattern);
                        }
                        NumberFormat::General
                        | NumberFormat::Decimal(_)
                        | NumberFormat::Percent(_)
                        | NumberFormat::Date
                        | NumberFormat::Time => {}
                    }
                }

                if let Some(CellValue::Text(value)) = &cell.calculated {
                    account_owned_string(&mut usage, &mut estimated_bytes, value);
                }
            }

            usage.records = usage.records.saturating_add(sheet.merges.len());
            estimated_bytes = estimated_bytes
                .saturating_add(estimated_btree_set_bytes::<CellRange>(sheet.merges.len()));

            let row_count = sheet.row_dimensions.len();
            let column_count = sheet.column_dimensions.len();
            usage.records = usage
                .records
                .saturating_add(row_count)
                .saturating_add(column_count);
            estimated_bytes = estimated_bytes
                .saturating_add(estimated_btree_map_bytes::<RowIndex, f64>(row_count))
                .saturating_add(estimated_btree_map_bytes::<ColumnIndex, f64>(column_count));

            let hidden_row_count = sheet.manual_hidden_rows.len();
            let hidden_column_count = sheet.manual_hidden_columns.len();
            usage.records = usage
                .records
                .saturating_add(hidden_row_count)
                .saturating_add(hidden_column_count);
            estimated_bytes = estimated_bytes
                .saturating_add(estimated_btree_set_bytes::<RowIndex>(hidden_row_count))
                .saturating_add(estimated_btree_set_bytes::<ColumnIndex>(
                    hidden_column_count,
                ));

            if let Some(filter) = &sheet.auto_filter {
                usage.records = usage.records.saturating_add(1);
                let criteria_count = filter.criteria.len();
                usage.records = usage.records.saturating_add(criteria_count);
                estimated_bytes = estimated_bytes
                    .saturating_add(estimated_btree_map_bytes::<ColumnIndex, FilterCriterion>(
                        criteria_count,
                    ));

                for criterion in filter.criteria.values() {
                    match criterion {
                        FilterCriterion::Values(values) => {
                            usage.records = usage.records.saturating_add(values.len());
                            estimated_bytes = estimated_bytes
                                .saturating_add(estimated_vec_bytes::<String>(values.capacity()));
                            for value in values {
                                account_owned_string(&mut usage, &mut estimated_bytes, value);
                            }
                        }
                        FilterCriterion::Equals(value)
                        | FilterCriterion::NotEquals(value)
                        | FilterCriterion::Contains(value) => {
                            account_owned_string(&mut usage, &mut estimated_bytes, value);
                        }
                        FilterCriterion::Blank
                        | FilterCriterion::NonBlank
                        | FilterCriterion::NumberEquals(_)
                        | FilterCriterion::GreaterThan(_)
                        | FilterCriterion::LessThan(_) => {}
                    }
                }
            }
        }

        usage.estimated_owned_bytes = estimated_bytes;
        usage
    }

    /// Returns cell input snapshot if present.
    pub fn cell_input(&self, sheet: SheetId, address: CellAddress) -> Option<&CellInput> {
        let sheet = self.sheet_by_id(sheet)?;
        sheet
            .cells
            .get(&(address.row, address.column))?
            .input
            .as_ref()
    }

    /// Returns calculated cell value if present.
    pub fn cell_value(&self, sheet: SheetId, address: CellAddress) -> Option<&CellValue> {
        let sheet = self.sheet_by_id(sheet)?;
        sheet
            .cells
            .get(&(address.row, address.column))?
            .calculated
            .as_ref()
    }

    /// Returns cell format if present.
    pub fn cell_format(&self, sheet: SheetId, address: CellAddress) -> Option<&CellFormat> {
        let sheet = self.sheet_by_id(sheet)?;
        sheet
            .cells
            .get(&(address.row, address.column))?
            .format
            .as_ref()
    }

    /// Returns a canonical row dimension.
    pub fn row_height(&self, sheet: SheetId, row: RowIndex) -> Option<f64> {
        self.sheet_by_id(sheet)
            .and_then(|s| s.row_dimensions.get(&row).copied())
    }

    /// Returns a canonical column width.
    pub fn column_width(&self, sheet: SheetId, column: ColumnIndex) -> Option<f64> {
        self.sheet_by_id(sheet)
            .and_then(|s| s.column_dimensions.get(&column).copied())
    }

    /// Returns whether address is contained in an existing merge.
    pub fn merge_at(&self, sheet: SheetId, address: CellAddress) -> Option<CellRange> {
        self.sheet_by_id(sheet)?
            .merges
            .iter()
            .find(|range| range.contains(&address))
            .cloned()
    }

    /// Query sparse cells in range in document order.
    pub fn query_range(&self, range: &CellRange) -> Vec<CellSnapshot> {
        self.sheet_by_id(range.sheet)
            .map(|sheet| {
                sheet
                    .cells
                    .iter()
                    .filter_map(|((row, col), cell)| {
                        let address = CellAddress::new(range.sheet, *row, *col);
                        if !range.contains(&address) {
                            return None;
                        }
                        if cell.input.is_none()
                            && cell.format.is_none()
                            && cell.calculated.is_none()
                        {
                            None
                        } else {
                            Some(CellSnapshot {
                                address,
                                input: cell.input.clone().unwrap_or(CellInput::Empty),
                                format: cell.format.clone(),
                            })
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Builds a read-only workbook snapshot for engine requests.
    pub fn snapshot(&self) -> WorkbookSnapshot {
        WorkbookSnapshot {
            revision: self.revision,
            active_sheet: self.active_sheet,
            sheets: self.sheets.iter().map(Sheet::to_snapshot).collect(),
        }
    }

    fn sheet_by_id(&self, sheet: SheetId) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.id == sheet)
    }

    fn sheet_by_id_mut(&mut self, sheet: SheetId) -> Option<&mut Sheet> {
        self.sheets.iter_mut().find(|s| s.id == sheet)
    }

    fn index_of_sheet(&self, sheet: SheetId) -> Option<usize> {
        self.sheets.iter().position(|s| s.id == sheet)
    }

    fn next_sheet_id(&self) -> Result<SheetId, CoreError> {
        let next = self
            .sheets
            .iter()
            .map(|sheet| sheet.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(CoreError::InvalidAddress(CellAddress::new(
                SheetId(u32::MAX),
                RowIndex(0),
                ColumnIndex(0),
            )))?;
        Ok(SheetId(next))
    }

    fn check_unique_sheet_name(
        &self,
        name: &str,
        except: Option<SheetId>,
    ) -> Result<(), CoreError> {
        let exists = self
            .sheets
            .iter()
            .any(|sheet| Some(sheet.id) != except && sheet.name.eq_ignore_ascii_case(name));
        if exists {
            Err(CoreError::DuplicateSheetName(name.to_string()))
        } else {
            Ok(())
        }
    }

    fn validate_range(range: &CellRange) -> Result<(), CoreError> {
        range.validate()
    }

    fn ensure_formula_input(input: &CellInput) -> Result<CellInput, CoreError> {
        match input {
            CellInput::Formula(raw) => CellInput::formula(raw.clone()),
            CellInput::Number(v) => Ok(CellInput::Number(ensure_finite_f64(*v, "Number")?)),
            CellInput::Percent(v) => Ok(CellInput::Percent(ensure_finite_f64(*v, "Percent")?)),
            CellInput::Date(v) => Ok(CellInput::Date(ensure_finite_f64(*v, "Date")?)),
            CellInput::Time(v) => Ok(CellInput::Time(ensure_finite_f64(*v, "Time")?)),
            other => Ok(other.clone()),
        }
    }

    fn validate_and_normalize_change(&self, change: &Change) -> Result<(), CoreError> {
        match change {
            Change::AddSheet { name } => {
                let candidate = match name {
                    Some(name) => name.clone(),
                    None => self.unique_sheet_name("Sheet"),
                };
                validate_sheet_name(&candidate)?;
                return self.check_unique_sheet_name(&candidate, None);
            }
            Change::RenameSheet { sheet, name } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                validate_sheet_name(name.as_str())?;
                self.check_unique_sheet_name(name, Some(*sheet))?;
            }
            Change::DuplicateSheet { sheet, name } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if let Some(target_name) = name {
                    validate_sheet_name(target_name)?;
                    self.check_unique_sheet_name(target_name, None)?;
                }
            }
            Change::SetCellInput {
                sheet,
                address,
                input,
            } => {
                if address.sheet != *sheet {
                    return Err(CoreError::InvalidAddress(*address));
                }
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                Self::ensure_formula_input(input)?;
            }
            Change::ClearCell { sheet, address } => {
                if address.sheet != *sheet {
                    return Err(CoreError::InvalidAddress(*address));
                }
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
            }
            Change::SetCellFormat { sheet, address, .. } => {
                if address.sheet != *sheet {
                    return Err(CoreError::InvalidAddress(*address));
                }
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
            }
            Change::ClearCellFormat { sheet, address } => {
                if address.sheet != *sheet {
                    return Err(CoreError::InvalidAddress(*address));
                }
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
            }
            Change::MergeCells { sheet, range } => {
                Self::validate_range(range)?;
                if range.sheet != *sheet {
                    return Err(CoreError::InvalidRange("merge range sheet mismatch".into()));
                }
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
            }
            Change::UnmergeCells { sheet, range } => {
                Self::validate_range(range)?;
                if range.sheet != *sheet {
                    return Err(CoreError::InvalidRange(
                        "unmerge range sheet mismatch".into(),
                    ));
                }
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
            }
            Change::SetRowDimension { sheet, row, height } => {
                if height.is_sign_negative() || !height.is_finite() || *row == RowIndex(u32::MAX) {
                    return Err(CoreError::InvalidFormula(
                        "invalid row index or height".into(),
                    ));
                }
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
            }
            Change::ClearRowDimension { sheet, .. } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
            }
            Change::SetColumnDimension { sheet, width, .. } => {
                if width.is_sign_negative() || !width.is_finite() {
                    return Err(CoreError::InvalidFormula("invalid column width".into()));
                }
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
            }
            Change::ClearColumnDimension { sheet, .. } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
            }
            Change::DeleteSheet { sheet } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if self.sheets.len() == 1 {
                    return Err(CoreError::CannotDeleteFinalSheet);
                }
            }
            Change::MoveSheet {
                sheet,
                target_index,
            } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if *target_index >= self.sheets.len() {
                    return Err(CoreError::InvalidRange(format!(
                        "target sheet index out of bounds: {}",
                        target_index
                    )));
                }
            }
            Change::SetActiveSheet { sheet } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
            }
            Change::SetAutoFilter { sheet, filter } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if let Some(filter) = filter {
                    filter.validate()?;
                    if filter.range.sheet != *sheet {
                        return Err(CoreError::InvalidRange(
                            "auto-filter range sheet mismatch".into(),
                        ));
                    }
                }
            }
            Change::SetManualHiddenRows { sheet, rows } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                let mut unique_rows = BTreeSet::new();
                for row in rows {
                    if row.0 == u32::MAX {
                        return Err(CoreError::InvalidRange(
                            "manual hidden row index must not be u32::MAX".into(),
                        ));
                    }
                    if !unique_rows.insert(*row) {
                        return Err(CoreError::InvalidRange(format!(
                            "duplicate manual hidden row: {row}"
                        )));
                    }
                }
            }
            Change::SetManualHiddenColumns { sheet, columns } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                let mut unique_columns = BTreeSet::new();
                for column in columns {
                    if column.0 == u32::MAX {
                        return Err(CoreError::InvalidRange(
                            "manual hidden column index must not be u32::MAX".into(),
                        ));
                    }
                    if !unique_columns.insert(*column) {
                        return Err(CoreError::InvalidRange(format!(
                            "duplicate manual hidden column: {column}"
                        )));
                    }
                }
            }
            Change::InsertRows { sheet, at, count } | Change::DeleteRows { sheet, at, count } => {
                if *count == 0 || at.0 as u64 + (*count as u64) > u32::MAX as u64 + 1 {
                    return Err(CoreError::InvalidRange(
                        "row operation overflows index space".into(),
                    ));
                }
                let target_sheet = self
                    .sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if target_sheet.auto_filter.is_some()
                    || !target_sheet.manual_hidden_rows.is_empty()
                    || !target_sheet.manual_hidden_columns.is_empty()
                {
                    return Err(CoreError::Unsupported(
                        "row operations while auto-filter or manual hidden metadata is active"
                            .into(),
                    ));
                }
            }
            Change::InsertColumns { sheet, at, count }
            | Change::DeleteColumns { sheet, at, count } => {
                if *count == 0 || at.0 as u64 + (*count as u64) > u32::MAX as u64 + 1 {
                    return Err(CoreError::InvalidRange(
                        "column operation overflows index space".into(),
                    ));
                }
                let target_sheet = self
                    .sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if target_sheet.auto_filter.is_some()
                    || !target_sheet.manual_hidden_rows.is_empty()
                    || !target_sheet.manual_hidden_columns.is_empty()
                {
                    return Err(CoreError::Unsupported(
                        "column operations while auto-filter or manual hidden metadata is active"
                            .into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn apply_change_sheet(&mut self, change: &Change) -> Result<bool, CoreError> {
        match change {
            Change::AddSheet { name } => {
                let candidate_name = name
                    .as_deref()
                    .filter(|n| !n.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| self.unique_sheet_name("Sheet"));
                let id = self.next_sheet_id()?;
                self.sheets.push(Sheet::new(id, candidate_name));
                self.active_sheet = id;
                Ok(true)
            }
            Change::DeleteSheet { sheet } => {
                let index = self
                    .index_of_sheet(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                self.sheets.remove(index);
                if self.active_sheet == *sheet {
                    let new_active = if index == 0 { 0 } else { index - 1 };
                    self.active_sheet = self.sheets[new_active].id;
                }
                Ok(true)
            }
            Change::RenameSheet { sheet, name } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                sheet.name = name.clone();
                Ok(true)
            }
            Change::MoveSheet {
                sheet,
                target_index,
            } => {
                let current = self
                    .index_of_sheet(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                let target = *target_index;
                if current == target {
                    return Ok(false);
                }
                let sheet_data = self.sheets.remove(current);
                self.sheets.insert(target, sheet_data);
                Ok(true)
            }
            Change::DuplicateSheet { sheet, name } => {
                let source_index = self
                    .index_of_sheet(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                let mut cloned = self.sheets[source_index].clone();
                let base_name = name
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| format!("{} Copy", self.sheets[source_index].name));
                let unique_name = self.ensure_unique_name(base_name);
                let new_id = self.next_sheet_id()?;
                cloned.id = new_id;
                cloned.name = unique_name;
                if let Some(filter) = cloned.auto_filter.as_mut() {
                    filter.range.sheet = new_id;
                    filter.range.start.sheet = new_id;
                    filter.range.end.sheet = new_id;
                }
                self.sheets.push(cloned);
                self.active_sheet = new_id;
                Ok(true)
            }
            Change::SetCellInput {
                sheet,
                address,
                input,
            } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                let normalized = Self::ensure_formula_input(input)?;
                if let Some(cell) = sheet.cells.get_mut(&(address.row, address.column)) {
                    cell.input = Some(normalized);
                    cell.calculated = None;
                    if cell.input == Some(CellInput::Empty) && cell.format.is_none() {
                        sheet.cells.remove(&(address.row, address.column));
                    }
                    return Ok(true);
                }
                if let CellInput::Empty = normalized {
                    return Ok(false);
                }
                sheet.cells.insert(
                    (address.row, address.column),
                    Cell {
                        input: Some(normalized),
                        format: None,
                        calculated: None,
                    },
                );
                Ok(true)
            }
            Change::ClearCell { sheet, address } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if let Some(existing) = sheet.cells.get_mut(&(address.row, address.column)) {
                    existing.input = Some(CellInput::Empty);
                    existing.calculated = None;
                    if existing.format.is_none() {
                        sheet.cells.remove(&(address.row, address.column));
                    }
                    return Ok(true);
                }
                Ok(false)
            }
            Change::SetCellFormat {
                sheet,
                address,
                format,
            } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if let Some(cell) = sheet.cells.get_mut(&(address.row, address.column)) {
                    cell.format = Some(format.clone());
                } else {
                    sheet.cells.insert(
                        (address.row, address.column),
                        Cell {
                            input: None,
                            format: Some(format.clone()),
                            calculated: None,
                        },
                    );
                }
                Ok(true)
            }
            Change::ClearCellFormat { sheet, address } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if let Some(cell) = sheet.cells.get_mut(&(address.row, address.column)) {
                    cell.format = None;
                    if cell.input.is_none() && cell.calculated.is_none() {
                        sheet.cells.remove(&(address.row, address.column));
                    }
                    return Ok(true);
                }
                Ok(false)
            }
            Change::MergeCells { sheet, range } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if sheet
                    .merges
                    .iter()
                    .any(|existing| existing.intersects(range))
                {
                    return Err(CoreError::InvalidRange(
                        "merge overlaps existing range".into(),
                    ));
                }
                sheet.merges.insert(range.clone());
                Ok(true)
            }
            Change::UnmergeCells { sheet, range } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if !sheet.merges.remove(range) {
                    return Err(CoreError::InvalidRange("merge range not found".into()));
                }
                Ok(true)
            }
            Change::SetRowDimension { sheet, row, height } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if !height.is_finite() || *height <= 0.0 {
                    return Err(CoreError::InvalidRange(
                        "row height must be positive".into(),
                    ));
                }
                sheet.row_dimensions.insert(*row, *height);
                Ok(true)
            }
            Change::ClearRowDimension { sheet, row } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                sheet.row_dimensions.remove(row);
                Ok(true)
            }
            Change::SetColumnDimension {
                sheet,
                column,
                width,
            } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if !width.is_finite() || *width <= 0.0 {
                    return Err(CoreError::InvalidRange(
                        "column width must be positive".into(),
                    ));
                }
                sheet.column_dimensions.insert(*column, *width);
                Ok(true)
            }
            Change::ClearColumnDimension { sheet, column } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                sheet.column_dimensions.remove(column);
                Ok(true)
            }
            Change::SetActiveSheet { sheet } => {
                self.sheet_by_id(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                if self.active_sheet == *sheet {
                    return Ok(false);
                }
                self.active_sheet = *sheet;
                Ok(true)
            }
            Change::SetAutoFilter { sheet, filter } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                sheet.auto_filter = filter.clone();
                Ok(true)
            }
            Change::SetManualHiddenRows { sheet, rows } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                sheet.manual_hidden_rows = rows.iter().copied().collect();
                Ok(true)
            }
            Change::SetManualHiddenColumns { sheet, columns } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                sheet.manual_hidden_columns = columns.iter().copied().collect();
                Ok(true)
            }
            Change::InsertRows { sheet, at, count } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                shift_rows(sheet, at.0, *count, false)?;
                Ok(true)
            }
            Change::DeleteRows { sheet, at, count } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                shift_rows(sheet, at.0, *count, true)?;
                Ok(true)
            }
            Change::InsertColumns { sheet, at, count } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                shift_columns(sheet, at.0, *count, false)?;
                Ok(true)
            }
            Change::DeleteColumns { sheet, at, count } => {
                let sheet = self
                    .sheet_by_id_mut(*sheet)
                    .ok_or(CoreError::SheetNotFound(*sheet))?;
                shift_columns(sheet, at.0, *count, true)?;
                Ok(true)
            }
        }
    }

    fn ensure_unique_name(&self, desired: String) -> String {
        if !self
            .sheets
            .iter()
            .any(|sheet| sheet.name.eq_ignore_ascii_case(&desired))
            && scalar_len(&desired) <= SHEET_NAME_MAX_SCALARS
        {
            return desired;
        }

        let mut counter = 1;
        loop {
            let suffix = format!(" ({counter})");
            let max_base = SHEET_NAME_MAX_SCALARS.saturating_sub(scalar_len(&suffix));
            let base = if scalar_len(&desired) > max_base {
                truncate_to_max_scalars(&desired)
                    .chars()
                    .take(max_base)
                    .collect::<String>()
            } else {
                desired.clone()
            };
            let candidate = format!("{base}{suffix}");
            if !self
                .sheets
                .iter()
                .any(|sheet| sheet.name.eq_ignore_ascii_case(&candidate))
            {
                return candidate;
            }
            counter += 1;
        }
    }

    fn unique_sheet_name(&self, prefix: &str) -> String {
        let mut candidate = 1;
        loop {
            let proposed = format!("{prefix}{candidate}");
            if !self
                .sheets
                .iter()
                .any(|sheet| sheet.name.eq_ignore_ascii_case(&proposed))
            {
                return proposed;
            }
            candidate += 1;
        }
    }

    /// Non-mutating validation and request preparation.
    pub fn prepare(&self, changes: ChangeSet) -> Result<MutationPlan, CoreError> {
        let mut candidate = self.clone();
        for change in &changes.changes {
            candidate.validate_and_normalize_change(change)?;
            candidate.apply_change_sheet(change)?;
        }
        if candidate.revision == Revision(u64::MAX) {
            return Err(CoreError::RevisionExhausted);
        }

        Ok(MutationPlan {
            base_revision: self.revision,
            workbook_token: self.workbook_token,
            request: EngineRequest {
                base_revision: self.revision,
                workbook: candidate.snapshot(),
            },
            candidate,
            committed: false,
        })
    }

    /// Commits previously prepared changes and a trusted engine response atomically.
    pub fn commit(
        &mut self,
        plan: &mut MutationPlan,
        response: EngineResponse,
    ) -> Result<(), CoreError> {
        if plan.committed {
            return Err(CoreError::PlanAlreadyCommitted);
        }
        if plan.workbook_token != self.workbook_token {
            return Err(CoreError::WorkbookMismatch);
        }
        if plan.base_revision != self.revision {
            return Err(CoreError::RevisionMismatch {
                expected: self.revision,
                got: plan.base_revision,
            });
        }
        if response.base_revision() != plan.base_revision {
            return Err(CoreError::RevisionMismatch {
                expected: plan.base_revision,
                got: response.base_revision(),
            });
        }

        let mut next = plan.candidate.clone();

        // Validate and apply calculated results only to formula cells.
        let mut touched = HashSet::new();
        for calc in response.calculated_cells() {
            if !touched.insert(calc.address) {
                return Err(CoreError::InvalidEngineResponse(
                    "duplicate address in engine response".into(),
                ));
            }

            let sheet = next
                .sheet_by_id_mut(calc.address.sheet)
                .ok_or(CoreError::InvalidAddress(calc.address))?;
            let key = (calc.address.row, calc.address.column);
            let cell = sheet
                .cells
                .get_mut(&key)
                .ok_or(CoreError::InvalidAddress(calc.address))?;
            match cell.input.as_ref() {
                Some(CellInput::Formula(_)) => {}
                _ => return Err(CoreError::ResponseForNonFormula(calc.address)),
            }

            match &calc.value {
                CellValue::Number(v)
                | CellValue::Percent(v)
                | CellValue::Date(v)
                | CellValue::Time(v)
                    if !v.is_finite() =>
                {
                    return Err(CoreError::NonFiniteNumericValue(
                        "engine returned non-finite number".into(),
                    ));
                }
                _ => {}
            }
            cell.calculated = Some(calc.value.clone());
        }

        next.revision = next.revision.checked_next()?;
        *self = next;
        plan.committed = true;
        Ok(())
    }

    /// Returns unsupported placeholder marker.
    pub fn unsupported_timezone_ops(&self) -> Result<(), CoreError> {
        Err(CoreError::Unsupported(
            "timezone operations are unsupported in core".into(),
        ))
    }
}

/// Forbidden characters for sheet names shared by core and XLSX boundary.
const FORBIDDEN_SHEET_CHARS: [char; 7] = ['[', ']', ':', '*', '?', '/', '\\'];
/// Maximum allowed sheet name length in Unicode scalar values.
pub const SHEET_NAME_MAX_SCALARS: usize = 31;

fn scalar_len(value: &str) -> usize {
    value.chars().count()
}

fn truncate_to_max_scalars(value: &str) -> String {
    value.chars().take(SHEET_NAME_MAX_SCALARS).collect()
}

/// Canonical sheet name validator.
///
/// Rules:
/// * trimmed input must not be empty;
/// * no forbidden characters `[]:*?/\\`;
/// * at most [`SHEET_NAME_MAX_SCALARS`] Unicode scalar values.
///
/// This helper intentionally keeps ASCII-only duplicate/case semantics out of
/// scope; use [`is_same_sheet_name`] for current-case-insensitive comparisons.
pub fn validate_sheet_name(name: &str) -> Result<(), CoreError> {
    if name.trim().is_empty() {
        return Err(CoreError::invalid_sheet_name("empty"));
    }
    if name
        .chars()
        .any(|character| FORBIDDEN_SHEET_CHARS.contains(&character))
    {
        return Err(CoreError::invalid_sheet_name(format!(
            "forbidden character in '{name}'"
        )));
    }
    if scalar_len(name) > SHEET_NAME_MAX_SCALARS {
        return Err(CoreError::invalid_sheet_name(format!(
            "name too long: {name} (max {SHEET_NAME_MAX_SCALARS})"
        )));
    }
    Ok(())
}

/// ASCII case-insensitive sheet-name equality helper.
///
/// Excel sheet-name uniqueness is evaluated case-insensitively only for the
/// ASCII subset.  This helper does not apply Unicode case folding, regex, or
/// normalization; callers must use exact text preservation for add/rename/duplicate.
pub fn is_same_sheet_name(current: &str, candidate: &str) -> bool {
    current.eq_ignore_ascii_case(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn a1(sheet: SheetId) -> CellAddress {
        CellAddress::new(sheet, RowIndex(0), ColumnIndex(0))
    }

    fn b2(sheet: SheetId) -> CellAddress {
        CellAddress::new(sheet, RowIndex(1), ColumnIndex(1))
    }

    #[test]
    fn range_validation_and_normalization() {
        let sheet = SheetId(1);
        let start = CellAddress::new(sheet, RowIndex(10), ColumnIndex(5));
        let end = CellAddress::new(sheet, RowIndex(2), ColumnIndex(1));
        let range = CellRange::new(start, end).expect("valid");
        assert_eq!(range.start.row.0, 2);
        assert_eq!(range.start.column.0, 1);
        assert_eq!(range.end.row.0, 10);
        assert_eq!(range.end.column.0, 5);
        assert_eq!(range.rows(), 9);
    }

    #[test]
    fn range_rejects_cross_sheet() {
        let start = CellAddress::new(SheetId(1), RowIndex(0), ColumnIndex(0));
        let end = CellAddress::new(SheetId(2), RowIndex(1), ColumnIndex(1));
        assert!(CellRange::new(start, end).is_err());
    }

    #[test]
    fn sheet_name_validation() {
        let wb = Workbook::new();
        let mut set = ChangeSet::new();
        set.push(Change::AddSheet {
            name: Some(String::new()),
        });
        assert!(wb.prepare(set).is_err());

        let mut set = ChangeSet::new();
        set.push(Change::AddSheet {
            name: Some("A/B".into()),
        });
        assert!(wb.prepare(set).is_err());

        let mut set = ChangeSet::new();
        set.push(Change::AddSheet {
            name: Some("ReallyLongSheetNameExceedingThirtyOneChars".into()),
        });
        assert!(wb.prepare(set).is_err());

        let mut set = ChangeSet::new();
        set.push(Change::AddSheet {
            name: Some("Sheet1".into()),
        });
        assert!(wb.prepare(set).is_err());
    }

    #[test]
    fn sparse_cell_storage_and_lookup() {
        let mut wb = Workbook::new();
        let sheet = wb.active_sheet();
        let address = a1(sheet);

        let mut change = ChangeSet::new();
        change.push(Change::SetCellInput {
            sheet,
            address,
            input: CellInput::Number(10.0),
        });
        let mut plan = wb.prepare(change).expect("valid set");
        wb.commit(&mut plan, EngineResponse::new(Revision(0), vec![]))
            .expect("commit");

        assert!(wb.cell_input(sheet, address).is_some());

        let mut clear = ChangeSet::new();
        clear.push(Change::ClearCell { sheet, address });
        let mut plan = wb.prepare(clear).expect("valid clear");
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .expect("commit clear");

        assert!(wb.cell_input(sheet, address).is_none());
        assert!(
            wb.query_range(&CellRange::new(address, address).expect("valid"))
                .is_empty()
        );
    }

    #[test]
    fn sheet_lifecycle_moves_and_duplicates() {
        let mut wb = Workbook::new();

        let mut add = ChangeSet::new();
        add.push(Change::AddSheet {
            name: Some("Data".into()),
        });
        let mut plan = wb.prepare(add).unwrap();
        wb.commit(&mut plan, EngineResponse::new(Revision(0), vec![]))
            .unwrap();

        let sheet2 = wb.active_sheet();
        let mut dup = ChangeSet::new();
        dup.push(Change::DuplicateSheet {
            sheet: sheet2,
            name: Some("DataCopy".into()),
        });
        let mut plan = wb.prepare(dup).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let sheet3 = wb.active_sheet();
        let mut rename = ChangeSet::new();
        rename.push(Change::RenameSheet {
            sheet: sheet3,
            name: "Renamed".into(),
        });
        let mut plan = wb.prepare(rename).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let before = wb.sheet_count();
        let move_target = 0;
        let mut mv = ChangeSet::new();
        mv.push(Change::MoveSheet {
            sheet: sheet3,
            target_index: move_target,
        });
        let mut plan = wb.prepare(mv).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        assert_eq!(wb.sheet_count(), before);
        assert_eq!(wb.sheet_name(wb.active_sheet()).unwrap(), "Renamed");

        let mut del = ChangeSet::new();
        del.push(Change::DeleteSheet { sheet: sheet2 });
        let mut plan = wb.prepare(del).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();
        assert!(wb.sheet_count() == before - 1);
        assert_ne!(wb.sheet_count(), 0);
    }

    #[test]
    fn formatting_and_dimensions() {
        let mut wb = Workbook::new();
        let sheet = wb.active_sheet();
        let addr = b2(sheet);

        let mut set = ChangeSet::new();
        set.push(Change::SetCellFormat {
            sheet,
            address: addr,
            format: CellFormat {
                font: Font {
                    family: "Times New Roman".into(),
                    size: 14,
                    bold: true,
                    italic: true,
                    underline: false,
                },
                number_format: NumberFormat::Percent(2),
                ..CellFormat::default()
            },
        });
        set.push(Change::SetRowDimension {
            sheet,
            row: RowIndex(1),
            height: 24.0,
        });
        set.push(Change::SetColumnDimension {
            sheet,
            column: ColumnIndex(1),
            width: 100.0,
        });
        let mut plan = wb.prepare(set).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let format = wb.cell_format(sheet, addr).expect("format");
        assert_eq!(format.font.family, "Times New Roman");
        assert_eq!(format.number_format, NumberFormat::Percent(2));
        assert_eq!(wb.row_height(sheet, RowIndex(1)), Some(24.0));
        assert_eq!(wb.column_width(sheet, ColumnIndex(1)), Some(100.0));
    }

    #[test]
    fn prepare_has_no_side_effects() {
        let wb = Workbook::new();
        let sheet = wb.active_sheet();
        let addr = a1(sheet);
        let mut set = ChangeSet::new();
        set.push(Change::SetCellInput {
            sheet,
            address: addr,
            input: CellInput::Number(PI),
        });

        let snapshot_before = wb.snapshot();
        let plan = wb.prepare(set).unwrap();
        assert!(wb.cell_input(sheet, addr).is_none());
        assert_eq!(wb.snapshot(), snapshot_before);

        let req = plan.request().workbook();
        assert_eq!(req.revision, wb.revision());
        assert_eq!(req.sheets.len(), wb.sheet_count());
        assert!(
            wb.query_range(&CellRange::new(addr, addr).unwrap())
                .is_empty()
        );
    }

    #[test]
    fn valid_commit_and_revision() {
        let mut wb = Workbook::new();
        let sheet = wb.active_sheet();
        let formula_cell = a1(sheet);

        let mut set = ChangeSet::new();
        set.push(Change::SetCellInput {
            sheet,
            address: formula_cell,
            input: CellInput::Formula("=A1+1".into()),
        });
        let mut plan = wb.prepare(set).unwrap();
        let req = plan.request();
        let response = EngineResponse::new(
            req.base_revision(),
            vec![CalculatedCell {
                address: formula_cell,
                value: CellValue::Number(42.0),
            }],
        );

        let old_revision = wb.revision();
        wb.commit(&mut plan, response).unwrap();
        assert_eq!(wb.revision(), old_revision.next().unwrap());
        assert_eq!(wb.revision(), Revision(1));
        assert_eq!(
            wb.cell_value(sheet, formula_cell),
            Some(&CellValue::Number(42.0))
        );
        assert_eq!(
            wb.cell_input(sheet, formula_cell),
            Some(&CellInput::Formula("A1+1".into()))
        );
    }

    #[test]
    fn stale_mismatched_and_double_commit_rejected() {
        let mut wb = Workbook::new();
        let sheet = wb.active_sheet();
        let formula_cell = a1(sheet);

        let mut set = ChangeSet::new();
        set.push(Change::SetCellInput {
            sheet,
            address: formula_cell,
            input: CellInput::Formula("=SUM(A1)".into()),
        });
        let mut plan = wb.prepare(set).unwrap();

        let mut change = ChangeSet::new();
        change.push(Change::SetCellInput {
            sheet,
            address: b2(sheet),
            input: CellInput::Number(5.0),
        });
        let mut concurrent = wb.prepare(change).unwrap();
        let concurrent_revision = concurrent.base_revision();
        wb.commit(
            &mut concurrent,
            EngineResponse::new(concurrent_revision, vec![]),
        )
        .unwrap();

        let response = EngineResponse::new(
            Revision(0),
            vec![CalculatedCell {
                address: formula_cell,
                value: CellValue::Number(1.0),
            }],
        );
        assert!(wb.commit(&mut plan, response).is_err());

        let _ignored_request = plan.engine_request();
        let mismatch = EngineResponse::new(
            Revision(999),
            vec![CalculatedCell {
                address: formula_cell,
                value: CellValue::Number(1.0),
            }],
        );
        assert!(wb.commit(&mut plan, mismatch).is_err());

        let mut fresh_change = ChangeSet::new();
        fresh_change.push(Change::SetCellInput {
            sheet,
            address: formula_cell,
            input: CellInput::Formula("=SUM(A1)".into()),
        });
        let mut fresh_plan = wb.prepare(fresh_change).unwrap();
        let response = EngineResponse::new(
            fresh_plan.base_revision(),
            vec![CalculatedCell {
                address: formula_cell,
                value: CellValue::Number(2.0),
            }],
        );
        wb.commit(&mut fresh_plan, response).unwrap();
        assert!(
            wb.commit(&mut fresh_plan, EngineResponse::new(wb.revision(), vec![]))
                .is_err()
        );
    }

    #[test]
    fn review_workbook_token_and_dependent_batch() {
        let mut first = Workbook::new();
        let mut second = Workbook::new();
        let mut changes = ChangeSet::new();
        changes.push(Change::AddSheet {
            name: Some("Batch".into()),
        });
        changes.push(Change::SetActiveSheet { sheet: SheetId(2) });
        let mut plan = first.prepare(changes).unwrap();
        assert_eq!(first.active_sheet(), SheetId(1));
        assert!(matches!(
            second.commit(&mut plan, EngineResponse::new(Revision(0), vec![])),
            Err(CoreError::WorkbookMismatch)
        ));
        first
            .commit(&mut plan, EngineResponse::new(Revision(0), vec![]))
            .unwrap();
        assert_eq!(first.active_sheet(), SheetId(2));

        let mut duplicate = ChangeSet::new();
        duplicate.push(Change::AddSheet {
            name: Some("Same".into()),
        });
        duplicate.push(Change::AddSheet {
            name: Some("Same".into()),
        });
        assert!(matches!(
            first.prepare(duplicate),
            Err(CoreError::DuplicateSheetName(_))
        ));
    }

    #[test]
    fn review_unicode_formula_and_revision_exhaustion() {
        let input = CellInput::formula(" =SUM(π1) ").unwrap();
        assert_eq!(input.as_formula(), Some("SUM(π1)"));
        assert!(matches!(
            Revision(u64::MAX).checked_next(),
            Err(CoreError::RevisionExhausted)
        ));
    }

    #[test]
    fn formula_length_is_bounded() {
        let error = CellInput::formula("1".repeat(MAX_FORMULA_BYTES + 1)).unwrap_err();
        assert!(matches!(
            error,
            CoreError::InvalidFormula(reason) if reason.contains("byte limit")
        ));
    }

    #[test]
    fn review_insert_delete_rows_and_columns_shift_sparse_state() {
        let mut wb = Workbook::new();
        let sheet = wb.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(2), ColumnIndex(2));
        let mut set = ChangeSet::new();
        set.push(Change::SetCellInput {
            sheet,
            address,
            input: CellInput::Text("kept".into()),
        });
        set.push(Change::SetRowDimension {
            sheet,
            row: RowIndex(2),
            height: 20.0,
        });
        set.push(Change::SetColumnDimension {
            sheet,
            column: ColumnIndex(2),
            width: 40.0,
        });
        let mut plan = wb.prepare(set).unwrap();
        wb.commit(&mut plan, EngineResponse::new(Revision(0), vec![]))
            .unwrap();

        let mut shift = ChangeSet::new();
        shift.push(Change::InsertRows {
            sheet,
            at: RowIndex(1),
            count: 2,
        });
        shift.push(Change::InsertColumns {
            sheet,
            at: ColumnIndex(1),
            count: 1,
        });
        let mut plan = wb.prepare(shift).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();
        let shifted = CellAddress::new(sheet, RowIndex(4), ColumnIndex(3));
        assert_eq!(
            wb.cell_input(sheet, shifted),
            Some(&CellInput::Text("kept".into()))
        );
        assert_eq!(wb.row_height(sheet, RowIndex(4)), Some(20.0));
        assert_eq!(wb.column_width(sheet, ColumnIndex(3)), Some(40.0));

        let mut remove = ChangeSet::new();
        remove.push(Change::DeleteRows {
            sheet,
            at: RowIndex(4),
            count: 1,
        });
        remove.push(Change::DeleteColumns {
            sheet,
            at: ColumnIndex(3),
            count: 1,
        });
        let mut plan = wb.prepare(remove).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();
        assert!(wb.cell_input(sheet, shifted).is_none());
    }

    #[test]
    fn review_malformed_public_range_is_rejected_at_merge_boundary() {
        let wb = Workbook::new();
        let sheet = wb.active_sheet();
        let mut malformed = CellRange::new(a1(sheet), b2(sheet)).unwrap();
        malformed.end = CellAddress::new(SheetId(999), RowIndex(1), ColumnIndex(1));
        let mut changes = ChangeSet::new();
        changes.push(Change::MergeCells {
            sheet,
            range: malformed,
        });
        assert!(matches!(
            wb.prepare(changes),
            Err(CoreError::InvalidRange(_))
        ));
    }

    #[test]
    fn fake_response_applies_formula_result() {
        let mut wb = Workbook::new();
        let sheet = wb.active_sheet();
        let formula_cell = a1(sheet);

        let mut set = ChangeSet::new();
        set.push(Change::SetCellInput {
            sheet,
            address: formula_cell,
            input: CellInput::Formula("=1+2".into()),
        });
        let mut plan = wb.prepare(set).unwrap();

        let response = EngineResponse::new(
            plan.base_revision(),
            vec![CalculatedCell {
                address: formula_cell,
                value: CellValue::Number(3.0),
            }],
        );

        wb.commit(&mut plan, response).unwrap();

        assert_eq!(
            wb.cell_input(sheet, formula_cell),
            Some(&CellInput::Formula("1+2".into()))
        );
        assert_eq!(
            wb.cell_value(sheet, formula_cell),
            Some(&CellValue::Number(3.0))
        );
    }

    #[test]
    fn sheet_name_validates_unicode_scalar_length() {
        // 31 Cyrillic scalars should be accepted.
        let valid_31 = "АБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮ"
            .chars()
            .collect::<Vec<_>>();
        assert_eq!(valid_31.len(), 31);
        let wb = Workbook::new();
        let mut set = ChangeSet::new();
        set.push(Change::AddSheet {
            name: Some(valid_31.iter().collect()),
        });
        assert!(wb.prepare(set).is_ok());

        // 32 Cyrillic scalars should be rejected.
        let invalid_32 = format!("{}А", valid_31.iter().collect::<String>());
        let mut set = ChangeSet::new();
        set.push(Change::AddSheet {
            name: Some(invalid_32),
        });
        assert!(wb.prepare(set).is_err());
    }

    #[test]
    fn sheet_name_accepts_multi_byte_scalars_within_limit() {
        let wb = Workbook::new();
        let mut set = ChangeSet::new();
        set.push(Change::AddSheet {
            name: Some("😀😁😂🤣😃😄😅😆".into()),
        });
        assert!(wb.prepare(set).is_ok());
    }

    #[test]
    fn sheet_name_rejects_blank_and_forbidden_characters() {
        let wb = Workbook::new();
        for invalid in ["", "   ", "[A]", "]A", "A:B", "A*B", "A?B", "A/B", "A\\B"] {
            let mut set = ChangeSet::new();
            set.push(Change::AddSheet {
                name: Some(invalid.into()),
            });
            assert!(
                wb.prepare(set).is_err(),
                "expected rejection for {invalid:?}"
            );
        }
    }

    #[test]
    fn sheet_name_duplicates_are_ascii_case_insensitive() {
        let mut wb = Workbook::new();
        let mut add = ChangeSet::new();
        add.push(Change::AddSheet {
            name: Some("Data".into()),
        });
        let mut plan = wb.prepare(add).unwrap();
        wb.commit(&mut plan, EngineResponse::new(Revision(0), vec![]))
            .unwrap();

        let mut dup = ChangeSet::new();
        dup.push(Change::AddSheet {
            name: Some("data".into()),
        });
        assert!(matches!(
            wb.prepare(dup),
            Err(CoreError::DuplicateSheetName(_))
        ));
    }

    #[test]
    fn sheet_name_preserves_whitespace_exactly() {
        let mut wb = Workbook::new();
        let mut add = ChangeSet::new();
        add.push(Change::AddSheet {
            name: Some(" leading".into()),
        });
        let mut plan = wb.prepare(add).unwrap();
        wb.commit(&mut plan, EngineResponse::new(Revision(0), vec![]))
            .unwrap();
        assert_eq!(wb.sheet_name(wb.active_sheet()), Some(" leading"));

        let sheet = wb.active_sheet();
        let mut rename = ChangeSet::new();
        rename.push(Change::RenameSheet {
            sheet,
            name: "trailing ".into(),
        });
        let mut plan = wb.prepare(rename).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();
        assert_eq!(wb.sheet_name(sheet), Some("trailing "));
    }

    #[test]
    fn auto_filter_and_manual_hidden_rows_roundtrip_and_duplicate_with_new_sheet_id() {
        let mut wb = Workbook::new();
        let source = wb.active_sheet();
        let mut criteria = BTreeMap::new();
        criteria.insert(
            ColumnIndex(1),
            FilterCriterion::Values(vec!["Open".into(), "Closed".into()]),
        );
        let filter = AutoFilter {
            range: CellRange::new(
                CellAddress::new(source, RowIndex(0), ColumnIndex(0)),
                CellAddress::new(source, RowIndex(4), ColumnIndex(2)),
            )
            .unwrap(),
            has_header: true,
            criteria,
        };

        let mut changes = ChangeSet::new();
        changes.push(Change::SetAutoFilter {
            sheet: source,
            filter: Some(filter.clone()),
        });
        changes.push(Change::SetManualHiddenRows {
            sheet: source,
            rows: vec![RowIndex(8), RowIndex(3)],
        });
        let mut plan = wb.prepare(changes).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let snapshot = wb.snapshot();
        assert_eq!(snapshot.sheets[0].auto_filter.as_ref(), Some(&filter));
        assert_eq!(
            snapshot.sheets[0].manual_hidden_rows,
            vec![RowIndex(3), RowIndex(8)]
        );
        let restored = Workbook::from_snapshot(snapshot.clone()).unwrap();
        assert_eq!(restored.snapshot(), snapshot);
        assert_eq!(wb.auto_filter(source), Some(&filter));
        let expected_hidden_rows = [RowIndex(3), RowIndex(8)]
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert_eq!(wb.manual_hidden_rows(source), Some(&expected_hidden_rows));

        let mut duplicate = ChangeSet::new();
        duplicate.push(Change::DuplicateSheet {
            sheet: source,
            name: Some("FilteredCopy".into()),
        });
        let mut plan = wb.prepare(duplicate).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();
        let copied = wb.active_sheet();
        let copied_filter = wb.auto_filter(copied).expect("copied filter");
        assert_eq!(copied_filter.range.sheet, copied);
        assert_eq!(copied_filter.range.start.sheet, copied);
        assert_eq!(copied_filter.range.end.sheet, copied);
        assert_eq!(copied_filter.has_header, filter.has_header);
        assert_eq!(copied_filter.criteria, filter.criteria);
        assert_eq!(wb.manual_hidden_rows(copied), Some(&expected_hidden_rows));
        assert_eq!(wb.auto_filter(source), Some(&filter));
    }

    #[test]
    fn manual_hidden_columns_roundtrip_and_duplicate_independently_from_rows() {
        let mut wb = Workbook::new();
        let source = wb.active_sheet();
        let mut changes = ChangeSet::new();
        changes.push(Change::SetManualHiddenRows {
            sheet: source,
            rows: vec![RowIndex(6)],
        });
        changes.push(Change::SetManualHiddenColumns {
            sheet: source,
            columns: vec![ColumnIndex(8), ColumnIndex(3)],
        });
        let mut plan = wb.prepare(changes).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let snapshot = wb.snapshot();
        assert_eq!(snapshot.sheets[0].manual_hidden_rows, vec![RowIndex(6)]);
        assert_eq!(
            snapshot.sheets[0].manual_hidden_columns,
            vec![ColumnIndex(3), ColumnIndex(8)]
        );
        let restored = Workbook::from_snapshot(snapshot.clone()).unwrap();
        assert_eq!(restored.snapshot(), snapshot);

        let expected_columns = [ColumnIndex(3), ColumnIndex(8)]
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert_eq!(wb.manual_hidden_columns(source), Some(&expected_columns));

        let mut duplicate = ChangeSet::new();
        duplicate.push(Change::DuplicateSheet {
            sheet: source,
            name: Some("HiddenCopy".into()),
        });
        let mut plan = wb.prepare(duplicate).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();
        let copied = wb.active_sheet();
        assert_eq!(wb.manual_hidden_columns(copied), Some(&expected_columns));
        let expected_rows = [RowIndex(6)].into_iter().collect::<BTreeSet<_>>();
        assert_eq!(wb.manual_hidden_rows(copied), Some(&expected_rows));
        assert_eq!(wb.manual_hidden_columns(source), Some(&expected_columns));
    }

    #[test]
    fn invalid_manual_hidden_columns_fail_atomically_and_snapshot_import_rejects_them() {
        let wb = Workbook::new();
        let sheet = wb.active_sheet();
        let before = wb.snapshot();

        for columns in [
            vec![ColumnIndex(1), ColumnIndex(1)],
            vec![ColumnIndex(u32::MAX)],
        ] {
            let mut changes = ChangeSet::new();
            changes.push(Change::SetManualHiddenColumns { sheet, columns });
            assert!(matches!(
                wb.prepare(changes),
                Err(CoreError::InvalidRange(_))
            ));
            assert_eq!(wb.snapshot(), before);
        }

        for columns in [
            vec![ColumnIndex(1), ColumnIndex(1)],
            vec![ColumnIndex(u32::MAX)],
        ] {
            let mut snapshot = before.clone();
            snapshot.sheets[0].manual_hidden_columns = columns;
            assert!(matches!(
                Workbook::from_snapshot(snapshot),
                Err(CoreError::InvalidSnapshot(_))
            ));
        }
    }

    #[test]
    fn manual_hidden_columns_block_row_and_column_structural_shifts() {
        let mut wb = Workbook::new();
        let sheet = wb.active_sheet();
        let mut changes = ChangeSet::new();
        changes.push(Change::SetManualHiddenColumns {
            sheet,
            columns: vec![ColumnIndex(4)],
        });
        let mut plan = wb.prepare(changes).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let before = wb.snapshot();
        for change in [
            Change::InsertRows {
                sheet,
                at: RowIndex(1),
                count: 1,
            },
            Change::DeleteColumns {
                sheet,
                at: ColumnIndex(0),
                count: 1,
            },
        ] {
            let changes = ChangeSet::new().with(change);
            assert!(matches!(
                wb.prepare(changes),
                Err(CoreError::Unsupported(_))
            ));
            assert_eq!(wb.snapshot(), before);
        }
    }

    #[test]
    fn filter_and_manual_row_validation_fail_without_changing_workbook() {
        let wb = Workbook::new();
        let sheet = wb.active_sheet();
        let before = wb.snapshot();

        let mut criteria = BTreeMap::new();
        criteria.insert(ColumnIndex(2), FilterCriterion::Equals("outside".into()));
        let invalid_filter = AutoFilter {
            range: CellRange::new(a1(sheet), b2(sheet)).unwrap(),
            has_header: true,
            criteria,
        };
        let mut changes = ChangeSet::new();
        changes.push(Change::SetAutoFilter {
            sheet,
            filter: Some(invalid_filter),
        });
        assert!(matches!(
            wb.prepare(changes),
            Err(CoreError::InvalidRange(_))
        ));
        assert_eq!(wb.snapshot(), before);

        for rows in [vec![RowIndex(1), RowIndex(1)], vec![RowIndex(u32::MAX)]] {
            let mut changes = ChangeSet::new();
            changes.push(Change::SetManualHiddenRows { sheet, rows });
            assert!(matches!(
                wb.prepare(changes),
                Err(CoreError::InvalidRange(_))
            ));
            assert_eq!(wb.snapshot(), before);
        }

        let mut invalid_criteria = BTreeMap::new();
        invalid_criteria.insert(ColumnIndex(0), FilterCriterion::GreaterThan(f64::INFINITY));
        let invalid_numeric_filter = AutoFilter {
            range: CellRange::new(a1(sheet), b2(sheet)).unwrap(),
            has_header: false,
            criteria: invalid_criteria,
        };
        assert!(matches!(
            invalid_numeric_filter.validate(),
            Err(CoreError::NonFiniteNumericValue(_))
        ));

        let other_sheet = SheetId(2);
        let mut mismatched_snapshot = before.clone();
        mismatched_snapshot.sheets[0].auto_filter = Some(AutoFilter {
            range: CellRange::new(a1(other_sheet), b2(other_sheet)).unwrap(),
            has_header: false,
            criteria: BTreeMap::new(),
        });
        assert!(matches!(
            Workbook::from_snapshot(mismatched_snapshot),
            Err(CoreError::InvalidRange(_))
        ));

        let mut duplicate_rows_snapshot = before.clone();
        duplicate_rows_snapshot.sheets[0].manual_hidden_rows = vec![RowIndex(2), RowIndex(2)];
        assert!(matches!(
            Workbook::from_snapshot(duplicate_rows_snapshot),
            Err(CoreError::InvalidSnapshot(_))
        ));
    }

    #[test]
    fn structural_shifts_are_unsupported_while_filter_state_is_active() {
        let mut wb = Workbook::new();
        let sheet = wb.active_sheet();
        let filter = AutoFilter {
            range: CellRange::new(a1(sheet), b2(sheet)).unwrap(),
            has_header: true,
            criteria: BTreeMap::new(),
        };
        let mut set_filter = ChangeSet::new();
        set_filter.push(Change::SetAutoFilter {
            sheet,
            filter: Some(filter),
        });
        let mut plan = wb.prepare(set_filter).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let before_shift = wb.snapshot();
        let mut shift_rows = ChangeSet::new();
        shift_rows.push(Change::InsertRows {
            sheet,
            at: RowIndex(1),
            count: 1,
        });
        assert!(matches!(
            wb.prepare(shift_rows),
            Err(CoreError::Unsupported(_))
        ));
        assert_eq!(wb.snapshot(), before_shift);

        let mut set_manual_rows = ChangeSet::new();
        set_manual_rows.push(Change::SetAutoFilter {
            sheet,
            filter: None,
        });
        set_manual_rows.push(Change::SetManualHiddenRows {
            sheet,
            rows: vec![RowIndex(4)],
        });
        let mut plan = wb.prepare(set_manual_rows).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let before_shift = wb.snapshot();
        let mut shift_columns = ChangeSet::new();
        shift_columns.push(Change::DeleteColumns {
            sheet,
            at: ColumnIndex(0),
            count: 1,
        });
        assert!(matches!(
            wb.prepare(shift_columns),
            Err(CoreError::Unsupported(_))
        ));
        assert_eq!(wb.snapshot(), before_shift);
    }

    #[test]
    fn snapshot_rejects_invalid_sheet_names() {
        let mut snapshot = WorkbookSnapshot {
            revision: Revision(0),
            active_sheet: SheetId(1),
            sheets: vec![
                SheetSnapshot {
                    id: SheetId(1),
                    name: "".into(),
                    cells: vec![],
                    merges: vec![],
                    row_dimensions: vec![],
                    column_dimensions: vec![],
                    auto_filter: None,
                    manual_hidden_rows: vec![],
                    manual_hidden_columns: vec![],
                },
                SheetSnapshot {
                    id: SheetId(2),
                    name: "Sheet2".into(),
                    cells: vec![],
                    merges: vec![],
                    row_dimensions: vec![],
                    column_dimensions: vec![],
                    auto_filter: None,
                    manual_hidden_rows: vec![],
                    manual_hidden_columns: vec![],
                },
            ],
        };
        assert!(Workbook::from_snapshot(snapshot.clone()).is_err());

        snapshot.sheets[0].name = "Valid".into();
        snapshot.sheets[0].cells = vec![CellSnapshot {
            address: CellAddress::new(SheetId(1), RowIndex(0), ColumnIndex(0)),
            input: CellInput::Empty,
            format: None,
        }];
        assert!(Workbook::from_snapshot(snapshot).is_ok());
    }

    #[test]
    fn duplicate_sheet_generated_name_stays_within_max_length() {
        let mut wb = Workbook::new();
        let long_valid_name = "А".repeat(31);
        let mut add = ChangeSet::new();
        add.push(Change::AddSheet {
            name: Some(long_valid_name.clone()),
        });
        let mut plan = wb.prepare(add).unwrap();
        wb.commit(&mut plan, EngineResponse::new(Revision(0), vec![]))
            .unwrap();

        let sheet = wb.active_sheet();
        let mut dup = ChangeSet::new();
        dup.push(Change::DuplicateSheet { sheet, name: None });
        let mut plan = wb.prepare(dup).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let new_sheet = wb.sheets.last().unwrap();
        assert!(new_sheet.name.chars().count() <= 31);
        assert_ne!(new_sheet.name, long_valid_name);
    }

    #[test]
    fn borrowed_sheet_metadata_views_preserve_workbook_state() {
        let mut wb = Workbook::new();
        let first = wb.active_sheet();
        let mut add_sheet = ChangeSet::new();
        add_sheet.push(Change::AddSheet {
            name: Some("Data".into()),
        });
        let mut plan = wb.prepare(add_sheet).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let first_merge = CellRange::new(a1(first), b2(first)).unwrap();
        let second_merge = CellRange::new(
            CellAddress::new(first, RowIndex(4), ColumnIndex(4)),
            CellAddress::new(first, RowIndex(5), ColumnIndex(5)),
        )
        .unwrap();
        let mut merges = ChangeSet::new();
        merges.push(Change::MergeCells {
            sheet: first,
            range: second_merge.clone(),
        });
        merges.push(Change::MergeCells {
            sheet: first,
            range: first_merge.clone(),
        });
        let mut plan = wb.prepare(merges).unwrap();
        wb.commit(&mut plan, EngineResponse::new(wb.revision(), vec![]))
            .unwrap();

        let before = wb.snapshot();
        let expected_merges = [first_merge, second_merge]
            .into_iter()
            .collect::<BTreeSet<_>>();
        let empty_merges = BTreeSet::new();

        assert_eq!(
            wb.sheet_names().collect::<Vec<_>>(),
            vec![(SheetId(1), "Sheet1"), (SheetId(2), "Data")]
        );
        assert_eq!(wb.sheet_merges(first), Some(&expected_merges));
        assert_eq!(wb.sheet_merges(SheetId(2)), Some(&empty_merges));
        assert_eq!(wb.sheet_merges(SheetId(999)), None);
        assert_eq!(wb.snapshot(), before);
    }

    #[test]
    fn resource_usage_accounts_for_cells_styles_filters_and_cached_text() {
        let mut wb = Workbook::new();
        let sheet = wb.active_sheet();
        let large_cell = CellAddress::new(sheet, RowIndex(50_000), ColumnIndex(120));
        let formula_cell = a1(sheet);
        let large_text = "L".repeat(4_096);
        let formula_text = "SUM(A1:A2)".to_owned();
        let cached_text = "cached-value-".repeat(256);
        let font_family = "Wide Font ".repeat(16);
        let custom_pattern = "0.000;".repeat(128);
        let currency_family = "Courier Long ".repeat(8);
        let currency_symbol = "руб. ".repeat(16);
        let filter_values = vec!["Open".to_owned(), "Closed".to_owned(), "Paused".to_owned()];
        let filter_text = "urgent".to_owned();
        let expected_text_bytes = "Sheet1".len()
            + large_text.len()
            + formula_text.len()
            + cached_text.len()
            + font_family.len()
            + custom_pattern.len()
            + currency_family.len()
            + currency_symbol.len()
            + filter_values.iter().map(String::len).sum::<usize>()
            + filter_text.len();

        let custom_format = CellFormat {
            font: Font {
                family: font_family,
                ..Font::default()
            },
            number_format: NumberFormat::Custom(custom_pattern),
            ..CellFormat::default()
        };
        let currency_format = CellFormat {
            font: Font {
                family: currency_family,
                ..Font::default()
            },
            number_format: NumberFormat::Currency {
                symbol: currency_symbol,
                precision: 2,
            },
            ..CellFormat::default()
        };

        let mut criteria = BTreeMap::new();
        criteria.insert(ColumnIndex(1), FilterCriterion::Values(filter_values));
        criteria.insert(ColumnIndex(2), FilterCriterion::Contains(filter_text));
        let filter = AutoFilter {
            range: CellRange::new(
                a1(sheet),
                CellAddress::new(sheet, RowIndex(5), ColumnIndex(2)),
            )
            .unwrap(),
            has_header: true,
            criteria,
        };
        let merge = CellRange::new(
            CellAddress::new(sheet, RowIndex(20), ColumnIndex(20)),
            CellAddress::new(sheet, RowIndex(20), ColumnIndex(21)),
        )
        .unwrap();

        let mut changes = ChangeSet::new();
        changes.push(Change::SetCellInput {
            sheet,
            address: large_cell,
            input: CellInput::Text(large_text),
        });
        changes.push(Change::SetCellFormat {
            sheet,
            address: large_cell,
            format: custom_format,
        });
        changes.push(Change::SetCellInput {
            sheet,
            address: formula_cell,
            input: CellInput::Formula(formula_text),
        });
        changes.push(Change::SetCellFormat {
            sheet,
            address: formula_cell,
            format: currency_format,
        });
        changes.push(Change::MergeCells {
            sheet,
            range: merge,
        });
        changes.push(Change::SetRowDimension {
            sheet,
            row: RowIndex(1_000),
            height: 21.0,
        });
        changes.push(Change::SetColumnDimension {
            sheet,
            column: ColumnIndex(1_000),
            width: 42.0,
        });
        changes.push(Change::SetManualHiddenRows {
            sheet,
            rows: vec![RowIndex(1_001)],
        });
        changes.push(Change::SetManualHiddenColumns {
            sheet,
            columns: vec![ColumnIndex(1_001)],
        });
        changes.push(Change::SetAutoFilter {
            sheet,
            filter: Some(filter),
        });

        let mut plan = wb.prepare(changes).unwrap();
        let base_revision = plan.base_revision();
        wb.commit(
            &mut plan,
            EngineResponse::new(
                base_revision,
                vec![CalculatedCell {
                    address: formula_cell,
                    value: CellValue::Text(cached_text),
                }],
            ),
        )
        .unwrap();

        let usage = wb.resource_usage();
        assert_eq!(usage.sheets, 1);
        assert_eq!(usage.records, 13);
        assert_eq!(usage.formulas, 1);
        assert_eq!(usage.owned_text_bytes, expected_text_bytes);
        assert!(usage.estimated_owned_bytes >= usage.owned_text_bytes);
        assert!(usage.estimated_owned_bytes > std::mem::size_of::<Workbook>());
    }
}
