//! Sheets application facade and bounded business logic.

mod clipboard;
#[cfg(test)]
mod clipboard_tests;
mod clipboard_tsv;
mod history;
pub use clipboard::{ClipboardPayload, MAX_CLIPBOARD_PAYLOAD_BYTES};
pub use clipboard_tsv::{MAX_CLIPBOARD_CELLS, MAX_CLIPBOARD_TSV_BYTES};
mod sort_filter;
#[cfg(test)]
mod sort_filter_history_budget_tests;
#[cfg(test)]
mod sort_filter_tests;
mod structural;
#[cfg(test)]
mod structural_tests;
use history::{History, HistoryEntry, Recording};
pub use history::{HistoryLimits, HistoryStats};
pub use sort_filter::{MAX_FILTER_TEXT_BYTES, MAX_SORT_FILTER_CELLS, SortOptions};
pub use structural::MAX_STRUCTURAL_CELLS;

use std::fmt;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

static NEXT_DOCUMENT_TOKEN: AtomicU64 = AtomicU64::new(1);

pub use sheets_core::CellInput;
pub use sheets_core::SpreadsheetEngine;
pub use sheets_core::{AutoFilter, FilterCriterion};
use sheets_core::{
    CellAddress, CellFormat, CellRange, CellSnapshot, CellValue, Change, ChangeSet, ColumnIndex,
    CoreError, EngineResponse, RowIndex, SheetId, Workbook, WorkbookSnapshot,
};
pub use sheets_ironcalc_adapter::{IroncalcAdapter, StructuralOperation, default_grid_metrics};
use sheets_xlsx::{MemoryPackage, XlsxCodec, ironcalc_codec, uses_bounded_import};

/// Convenience alias for the application-level result.
pub type ApiResult<T> = Result<T, ApiError>;

/// Application-level error wrapper.
#[derive(Debug)]
pub struct ApiError {
    kind: ApiErrorKind,
}

#[derive(Debug)]
pub enum ApiErrorKind {
    Core(CoreError),
    Transaction(ApiTransactionError),
    Parse(ApiParseError),
    Unsupported(ApiUnsupportedError),
    Format(ApiFormatError),
}

#[derive(Debug)]
pub enum ApiTransactionError {
    StaleRevision,
    MismatchedRevision,
    InvalidDocumentConfig(String),
    DocumentLimitExceeded(String),
    HistoryBudgetExceeded {
        required_cell_endpoints: usize,
        required_bytes: usize,
        limits: HistoryLimits,
    },
    EngineFailure(String),
    InvalidResponse(String),
}

#[derive(Debug)]
pub enum ApiParseError {
    EmptyInput,
    InvalidFormula(String),
    UnsupportedLiteral(String),
    InvalidNumber(String),
    InvalidDate(String),
    InvalidTime(String),
}

#[derive(Debug)]
pub enum ApiUnsupportedError {
    NativeImport,
    SortApply,
    LargeFormatMaterialization,
    FormulaRebasing,
    MergeWouldConcealInput { address: CellAddress },
    MergeOverlapsExisting { existing: CellRange },
    SortMergedRange { range: CellRange },
    SortFormula(String),
    CrossSheetHistory { sheet: SheetId },
    StructuralEdit(String),
    HistoryDependencyConflict { sheet: SheetId },
    Clipboard(String),
}

impl ApiError {
    pub fn kind(&self) -> &ApiErrorKind {
        &self.kind
    }

    pub fn is_core(&self) -> bool {
        matches!(self.kind, ApiErrorKind::Core(_))
    }

    pub fn as_core_error(&self) -> Option<&CoreError> {
        match self.kind() {
            ApiErrorKind::Core(error) => Some(error),
            _ => None,
        }
    }
}

impl From<CoreError> for ApiError {
    fn from(value: CoreError) -> Self {
        Self {
            kind: ApiErrorKind::Core(value),
        }
    }
}

impl From<ApiTransactionError> for ApiError {
    fn from(value: ApiTransactionError) -> Self {
        Self {
            kind: ApiErrorKind::Transaction(value),
        }
    }
}

impl From<ApiParseError> for ApiError {
    fn from(value: ApiParseError) -> Self {
        Self {
            kind: ApiErrorKind::Parse(value),
        }
    }
}

impl From<ApiUnsupportedError> for ApiError {
    fn from(value: ApiUnsupportedError) -> Self {
        Self {
            kind: ApiErrorKind::Unsupported(value),
        }
    }
}

impl fmt::Display for ApiParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "empty input"),
            Self::InvalidFormula(reason) => write!(f, "invalid formula: {reason}"),
            Self::UnsupportedLiteral(value) => write!(f, "unsupported literal: {value}"),
            Self::InvalidNumber(value) => write!(f, "invalid number: {value}"),
            Self::InvalidDate(value) => write!(f, "invalid date: {value}"),
            Self::InvalidTime(value) => write!(f, "invalid time: {value}"),
        }
    }
}

impl fmt::Display for ApiUnsupportedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NativeImport => {
                write!(f, "native import requires explicit typed restore support")
            }
            Self::SortApply => write!(f, "sort apply requires canonical row sort support"),
            Self::LargeFormatMaterialization => {
                write!(f, "large format materialization is not supported")
            }
            Self::FormulaRebasing => {
                write!(
                    f,
                    "structural rebasing requires formula-aware engine support"
                )
            }
            Self::MergeWouldConcealInput { address } => write!(
                f,
                "merge would conceal input at row {}, column {} on sheet {}",
                u64::from(address.row.0) + 1,
                u64::from(address.column.0) + 1,
                address.sheet.0
            ),
            Self::MergeOverlapsExisting { existing } => write!(
                f,
                "merge conflicts with existing range at rows {}..={}, columns {}..={} on sheet {}",
                u64::from(existing.start.row.0) + 1,
                u64::from(existing.end.row.0) + 1,
                u64::from(existing.start.column.0) + 1,
                u64::from(existing.end.column.0) + 1,
                existing.sheet.0
            ),
            Self::SortMergedRange { range } => {
                write!(f, "sort range intersects merged cells: {range:?}")
            }
            Self::SortFormula(reason) => {
                write!(f, "sort formula relocation is unsupported: {reason}")
            }
            Self::CrossSheetHistory { sheet } => write!(
                f,
                "latest history action for sheet {sheet} spans multiple sheets; use document undo/redo"
            ),
            Self::StructuralEdit(reason) => {
                write!(f, "structural edit is unsupported: {reason}")
            }
            Self::HistoryDependencyConflict { sheet } => write!(
                f,
                "history replay conflicts with a later dependent edit on sheet {sheet}"
            ),
            Self::Clipboard(reason) => write!(f, "clipboard operation is unsupported: {reason}"),
        }
    }
}

impl fmt::Display for ApiTransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleRevision => write!(f, "stale revision"),
            Self::MismatchedRevision => write!(f, "mismatched revision"),
            Self::InvalidDocumentConfig(reason) => {
                write!(f, "invalid document configuration: {reason}")
            }
            Self::DocumentLimitExceeded(reason) => {
                write!(f, "document limit exceeded: {reason}")
            }
            Self::HistoryBudgetExceeded {
                required_cell_endpoints,
                required_bytes,
                limits,
            } => write!(
                f,
                "undo history budget exceeded: transaction requires {required_cell_endpoints} cell endpoints and {required_bytes} bytes; limits are {} entries, {} cell endpoints and {} bytes",
                limits.max_entries, limits.max_retained_cells, limits.max_retained_bytes
            ),
            Self::EngineFailure(reason) => write!(f, "engine failure: {reason}"),
            Self::InvalidResponse(reason) => write!(f, "invalid response: {reason}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiFormatError {
    EmptyFontFamily,
    InvalidFontSize(String),
    InvalidHorizontalAlignment(String),
    InvalidVerticalAlignment(String),
    InvalidNumberFormat(String),
}

impl fmt::Display for ApiFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFontFamily => write!(f, "font family must not be empty"),
            Self::InvalidFontSize(reason) => write!(f, "invalid font size: {reason}"),
            Self::InvalidHorizontalAlignment(value) => {
                write!(f, "invalid horizontal alignment: {value}")
            }
            Self::InvalidVerticalAlignment(value) => {
                write!(f, "invalid vertical alignment: {value}")
            }
            Self::InvalidNumberFormat(value) => write!(f, "invalid number format: {value}"),
        }
    }
}

impl std::error::Error for ApiFormatError {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

impl From<Rgba8> for sheets_core::Color {
    fn from(value: Rgba8) -> Self {
        Self::new(value.r, value.g, value.b, value.a)
    }
}

impl From<sheets_core::Color> for Rgba8 {
    fn from(value: sheets_core::Color) -> Self {
        Self {
            r: value.r,
            g: value.g,
            b: value.b,
            a: value.a,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormatPatch {
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub text_color: Option<Rgba8>,
    pub background: Option<Rgba8>,
    pub horizontal_alignment: Option<String>,
    pub vertical_alignment: Option<String>,
    pub wrap: Option<bool>,
    pub number_format: Option<String>,
}

impl FormatPatch {
    pub fn is_empty(&self) -> bool {
        self.font_family.is_none()
            && self.font_size.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.underline.is_none()
            && self.text_color.is_none()
            && self.background.is_none()
            && self.horizontal_alignment.is_none()
            && self.vertical_alignment.is_none()
            && self.wrap.is_none()
            && self.number_format.is_none()
    }

    fn validated(&self) -> ApiResult<ValidatedFormatPatch> {
        if let Some(family) = self.font_family.as_ref() {
            let trimmed = family.trim();
            if trimmed.is_empty() {
                return Err(ApiFormatError::EmptyFontFamily.into());
            }
        }

        let font_size = if let Some(size) = self.font_size {
            if !size.is_finite() || size <= 0.0 || size.fract() != 0.0 || size > u16::MAX as f64 {
                return Err(ApiFormatError::InvalidFontSize(format!(
                    "font_size must be finite, positive, integral and <= {}",
                    u16::MAX
                ))
                .into());
            }
            Some(size as u16)
        } else {
            None
        };

        let horizontal_alignment = if let Some(value) = self.horizontal_alignment.as_ref() {
            match value.trim().to_lowercase().as_str() {
                "left" | "center" | "right" => Some(parse_horizontal_alignment(value.trim())),
                _ => {
                    return Err(ApiFormatError::InvalidHorizontalAlignment(value.clone()).into());
                }
            }
        } else {
            None
        };

        let vertical_alignment = if let Some(value) = self.vertical_alignment.as_ref() {
            match value.trim().to_lowercase().as_str() {
                "top" | "middle" | "bottom" => Some(parse_vertical_alignment(value.trim())),
                _ => {
                    return Err(ApiFormatError::InvalidVerticalAlignment(value.clone()).into());
                }
            }
        } else {
            None
        };

        let number_format = if let Some(value) = self.number_format.as_ref() {
            Some(parse_number_format(value)?)
        } else {
            None
        };

        Ok(ValidatedFormatPatch {
            font_family: self.font_family.clone(),
            font_size,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            text_color: self.text_color,
            background: self.background,
            horizontal_alignment,
            vertical_alignment,
            wrap: self.wrap,
            number_format,
        })
    }
}

#[derive(Debug, Clone)]
struct ValidatedFormatPatch {
    font_family: Option<String>,
    font_size: Option<u16>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    text_color: Option<Rgba8>,
    background: Option<Rgba8>,
    horizontal_alignment: Option<sheets_core::HorizontalAlign>,
    vertical_alignment: Option<sheets_core::VerticalAlign>,
    wrap: Option<bool>,
    number_format: Option<sheets_core::NumberFormat>,
}

impl ValidatedFormatPatch {
    fn is_empty(&self) -> bool {
        self.font_family.is_none()
            && self.font_size.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.underline.is_none()
            && self.text_color.is_none()
            && self.background.is_none()
            && self.horizontal_alignment.is_none()
            && self.vertical_alignment.is_none()
            && self.wrap.is_none()
            && self.number_format.is_none()
    }

    fn merge_into(&self, base: &sheets_core::CellFormat) -> sheets_core::CellFormat {
        let mut out = base.clone();
        if let Some(v) = &self.font_family {
            out.font.family = v.clone();
        }
        if let Some(v) = self.font_size {
            out.font.size = v;
        }
        if let Some(v) = self.bold {
            out.font.bold = v;
        }
        if let Some(v) = self.italic {
            out.font.italic = v;
        }
        if let Some(v) = self.underline {
            out.font.underline = v;
        }
        if let Some(v) = self.text_color {
            out.foreground = Some(v.into());
        }
        if let Some(v) = self.background {
            out.background = Some(v.into());
        }
        if let Some(v) = &self.horizontal_alignment {
            out.alignment.horizontal = Some(v.clone());
        }
        if let Some(v) = &self.vertical_alignment {
            out.alignment.vertical = Some(v.clone());
        }
        if let Some(v) = self.wrap {
            out.alignment.wrap = v;
        }
        if let Some(v) = &self.number_format {
            out.number_format = v.clone();
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedFormatPatchReference<'a>(&'a FormatPatch);

impl ApiError {
    pub fn is_format(&self) -> bool {
        matches!(self.kind, ApiErrorKind::Format(_))
    }
}

impl From<ApiFormatError> for ApiError {
    fn from(value: ApiFormatError) -> Self {
        Self {
            kind: ApiErrorKind::Format(value),
        }
    }
}

fn parse_horizontal_alignment(value: &str) -> sheets_core::HorizontalAlign {
    match value.trim().to_lowercase().as_str() {
        "left" => sheets_core::HorizontalAlign::Left,
        "center" => sheets_core::HorizontalAlign::Center,
        "right" => sheets_core::HorizontalAlign::Right,
        _ => sheets_core::HorizontalAlign::Left,
    }
}

fn parse_vertical_alignment(value: &str) -> sheets_core::VerticalAlign {
    match value.trim().to_lowercase().as_str() {
        "top" => sheets_core::VerticalAlign::Top,
        "middle" => sheets_core::VerticalAlign::Middle,
        "bottom" => sheets_core::VerticalAlign::Bottom,
        _ => sheets_core::VerticalAlign::Top,
    }
}

fn parse_number_format(value: &str) -> ApiResult<sheets_core::NumberFormat> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiFormatError::InvalidNumberFormat("empty format".into()).into());
    }
    let (kind, rest) = match trimmed.split_once(':') {
        Some(parts) => parts,
        None => (trimmed, ""),
    };
    match kind.trim().to_lowercase().as_str() {
        "general" => {
            if !rest.is_empty() {
                return Err(ApiFormatError::InvalidNumberFormat(trimmed.into()).into());
            }
            Ok(sheets_core::NumberFormat::General)
        }
        "date" => {
            if !rest.is_empty() {
                return Err(ApiFormatError::InvalidNumberFormat(trimmed.into()).into());
            }
            Ok(sheets_core::NumberFormat::Date)
        }
        "time" => {
            if !rest.is_empty() {
                return Err(ApiFormatError::InvalidNumberFormat(trimmed.into()).into());
            }
            Ok(sheets_core::NumberFormat::Time)
        }
        "decimal" | "percent" => {
            if rest.is_empty() {
                return Err(ApiFormatError::InvalidNumberFormat(trimmed.into()).into());
            }
            let precision =
                parse_u8(rest).map_err(|_| ApiFormatError::InvalidNumberFormat(trimmed.into()))?;
            if kind.trim().to_lowercase().as_str() == "decimal" {
                Ok(sheets_core::NumberFormat::Decimal(precision))
            } else {
                Ok(sheets_core::NumberFormat::Percent(precision))
            }
        }
        "currency" => {
            let mut currency_parts = rest.splitn(2, ':');
            let symbol = currency_parts
                .next()
                .ok_or_else(|| ApiFormatError::InvalidNumberFormat(trimmed.into()))?;
            if symbol.is_empty() {
                return Err(ApiFormatError::InvalidNumberFormat(trimmed.into()).into());
            }
            let precision = currency_parts
                .next()
                .ok_or_else(|| ApiFormatError::InvalidNumberFormat(trimmed.into()))?;
            let precision = parse_u8(precision)
                .map_err(|_| ApiFormatError::InvalidNumberFormat(trimmed.into()))?;
            Ok(sheets_core::NumberFormat::Currency {
                symbol: symbol.to_string(),
                precision,
            })
        }
        "custom" => {
            if rest.is_empty() {
                return Err(ApiFormatError::InvalidNumberFormat(trimmed.into()).into());
            }
            Ok(sheets_core::NumberFormat::Custom(rest.into()))
        }
        _ => Err(ApiFormatError::InvalidNumberFormat(trimmed.into()).into()),
    }
}

fn parse_u8(value: &str) -> Result<u8, ()> {
    if value.is_empty() {
        return Err(());
    }
    let parsed = value.parse::<u8>().map_err(|_| ())?;
    Ok(parsed)
}

impl fmt::Display for ApiErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => fmt::Display::fmt(error, f),
            Self::Transaction(error) => fmt::Display::fmt(error, f),
            Self::Parse(error) => fmt::Display::fmt(error, f),
            Self::Unsupported(error) => fmt::Display::fmt(error, f),
            Self::Format(error_kind) => fmt::Display::fmt(error_kind, f),
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl std::error::Error for ApiError {}

/// View layer used by the UI to render one canonical cell.
#[derive(Clone, Debug, PartialEq)]
pub struct CellView {
    pub address: CellAddress,
    pub input: CellInput,
    pub value: Option<CellValue>,
    pub format: Option<CellFormat>,
}

impl CellView {
    pub const fn new(
        address: CellAddress,
        input: CellInput,
        value: Option<CellValue>,
        format: Option<CellFormat>,
    ) -> Self {
        Self {
            address,
            input,
            value,
            format,
        }
    }

    /// Returns the effective display value, preferring calculated value over raw input.
    pub fn effective_value(&self) -> CellValue {
        self.value.clone().unwrap_or_else(|| match &self.input {
            CellInput::Empty => CellValue::Empty,
            CellInput::Text(value) => CellValue::Text(value.clone()),
            CellInput::Number(value) => CellValue::Number(*value),
            CellInput::Percent(value) => CellValue::Percent(*value),
            CellInput::Date(value) => CellValue::Date(*value),
            CellInput::Time(value) => CellValue::Time(*value),
            CellInput::Boolean(value) => CellValue::Boolean(*value),
            CellInput::Formula(_) => CellValue::Empty,
        })
    }

    /// Returns the display string for UI rendering.
    pub fn display_text(&self) -> String {
        if let Some(format) = &self.format
            && let Some(display) = sheets_ironcalc_adapter::formatted_numeric_display(
                &self.effective_value(),
                &format.number_format,
            )
        {
            return display;
        }
        cell_input_to_display(&self.input, &self.value)
    }

    /// Returns the raw input string for diagnostic or editing contexts.
    pub fn raw_text(&self) -> String {
        cell_input_to_raw(&self.input)
    }
}

/// Document configuration shared by application-level features.
#[derive(Clone, Debug)]
pub struct DocumentConfig {
    pub max_document_rows: u32,
    pub max_document_columns: u32,
    pub default_row_height: u16,
    pub default_column_width: u16,
}

impl Default for DocumentConfig {
    fn default() -> Self {
        let metrics = default_grid_metrics();
        Self {
            max_document_rows: 1_048_576,
            max_document_columns: 16_384,
            default_row_height: metrics.row_height.round() as u16,
            default_column_width: metrics.column_width.round() as u16,
        }
    }
}

impl DocumentConfig {
    pub const fn new(
        max_document_rows: u32,
        max_document_columns: u32,
        default_row_height: u16,
        default_column_width: u16,
    ) -> Self {
        Self {
            max_document_rows,
            max_document_columns,
            default_row_height,
            default_column_width,
        }
    }

    pub fn max_row(&self) -> RowIndex {
        RowIndex(self.max_document_rows.saturating_sub(1))
    }

    pub fn max_column(&self) -> ColumnIndex {
        ColumnIndex(self.max_document_columns.saturating_sub(1))
    }

    /// Validates that the document has an addressable grid.
    pub fn validate(&self) -> ApiResult<()> {
        if self.max_document_rows == 0 {
            return Err(ApiTransactionError::InvalidDocumentConfig(
                "max_document_rows must be greater than zero".into(),
            )
            .into());
        }
        if self.max_document_columns == 0 {
            return Err(ApiTransactionError::InvalidDocumentConfig(
                "max_document_columns must be greater than zero".into(),
            )
            .into());
        }
        Ok(())
    }
    pub fn normalize_row(&self, row: u32) -> ApiResult<RowIndex> {
        self.validate()?;
        if row >= self.max_document_rows {
            return Err(ApiTransactionError::DocumentLimitExceeded(format!(
                "row {row} exceeds document limit"
            ))
            .into());
        }
        Ok(RowIndex(row))
    }

    pub fn normalize_column(&self, column: u32) -> ApiResult<ColumnIndex> {
        self.validate()?;
        if column >= self.max_document_columns {
            return Err(ApiTransactionError::DocumentLimitExceeded(format!(
                "column {column} exceeds document limit"
            ))
            .into());
        }
        Ok(ColumnIndex(column))
    }
}

impl DocumentConfig {
    fn validate_address(&self, address: CellAddress) -> ApiResult<()> {
        self.normalize_row(address.row.0)?;
        self.normalize_column(address.column.0)?;
        Ok(())
    }

    fn validate_range(&self, range: &CellRange) -> ApiResult<()> {
        self.validate_address(range.start)?;
        self.validate_address(range.end)
    }

    fn validate_changes(&self, changes: &ChangeSet) -> ApiResult<()> {
        self.validate()?;
        for change in &changes.changes {
            match change {
                Change::SetCellInput { address, .. }
                | Change::ClearCell { address, .. }
                | Change::SetCellFormat { address, .. }
                | Change::ClearCellFormat { address, .. } => self.validate_address(*address)?,
                Change::MergeCells { range, .. } | Change::UnmergeCells { range, .. } => {
                    self.validate_range(range)?
                }
                Change::SetRowDimension { row, .. }
                | Change::ClearRowDimension { row, .. }
                | Change::InsertRows { at: row, .. }
                | Change::DeleteRows { at: row, .. } => {
                    self.normalize_row(row.0)?;
                }
                Change::SetColumnDimension { column, .. }
                | Change::ClearColumnDimension { column, .. }
                | Change::InsertColumns { at: column, .. }
                | Change::DeleteColumns { at: column, .. } => {
                    self.normalize_column(column.0)?;
                }
                Change::AddSheet { .. }
                | Change::DeleteSheet { .. }
                | Change::RenameSheet { .. }
                | Change::MoveSheet { .. }
                | Change::DuplicateSheet { .. }
                | Change::SetActiveSheet { .. } => {}
                Change::SetAutoFilter { filter, .. } => {
                    if let Some(filter) = filter {
                        self.validate_range(&filter.range)?;
                        sort_filter::validate_filter_payload(filter)?;
                    }
                }
                Change::SetManualHiddenRows { rows, .. } => {
                    for row in rows {
                        self.normalize_row(row.0)?;
                    }
                }
                Change::SetManualHiddenColumns { columns, .. } => {
                    for column in columns {
                        self.normalize_column(column.0)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_snapshot(&self, snapshot: &WorkbookSnapshot) -> ApiResult<()> {
        self.validate()?;
        for sheet in &snapshot.sheets {
            if let Some(filter) = &sheet.auto_filter {
                self.validate_range(&filter.range)?;
                sort_filter::validate_filter_payload(filter)?;
            }
            for row in &sheet.manual_hidden_rows {
                self.normalize_row(row.0)?;
            }
            for column in &sheet.manual_hidden_columns {
                self.normalize_column(column.0)?;
            }
            for cell in &sheet.cells {
                self.validate_address(cell.address)?;
            }
            for range in &sheet.merges {
                self.validate_range(range)?;
            }
            for row in &sheet.row_dimensions {
                self.normalize_row(row.row.0)?;
            }
            for column in &sheet.column_dimensions {
                self.normalize_column(column.column.0)?;
            }
        }
        Ok(())
    }
}

/// Sheet-level summary used in ordered tabs and status views.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetSummary {
    pub id: SheetId,
    pub name: String,
    pub active: bool,
    pub used_cells: u64,
    pub merges: u64,
}

/// Selection state returned to the UI layer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionSummary {
    pub sheet: SheetId,
    pub anchor: Option<CellAddress>,
    pub focus: Option<CellAddress>,
    pub range: Option<CellRange>,
    pub mode: SelectionMode,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SelectionMode {
    #[default]
    None,
    SingleCell,
    Range,
    Sheet,
}

/// Describes a single found cell.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchMatch {
    pub sheet: SheetId,
    pub address: CellAddress,
    pub input: CellInput,
    pub value: Option<CellValue>,
}

/// Row-level result after filter/sort preview projection.
#[derive(Clone, Debug, PartialEq)]
pub struct RowProjection {
    pub row: RowIndex,
    pub normalized_cells: Vec<CellSnapshot>,
}

/// Bounds applied to range formatting and content clearing before mutation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormatPatchBounds {
    pub max_cells: u64,
    pub max_sheets: usize,
}

/// Filter/sort preview parameters.
#[derive(Clone, Debug)]
pub struct FilterSortPreview {
    pub filter_predicate: fn(&CellSnapshot) -> bool,
    pub sort_key: fn(&CellSnapshot) -> (bool, String),
    pub ascending: bool,
    pub limit: Option<u64>,
}

impl FilterSortPreview {
    pub const fn new(
        filter_predicate: fn(&CellSnapshot) -> bool,
        sort_key: fn(&CellSnapshot) -> (bool, String),
        ascending: bool,
        limit: Option<u64>,
    ) -> Self {
        Self {
            filter_predicate,
            sort_key,
            ascending,
            limit,
        }
    }
}

/// Parsed formula bar entry.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedEntry {
    pub kind: EntryKind,
    pub raw: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EntryKind {
    Empty,
    Text(String),
    Number(f64),
    Percent(f64),
    Boolean(bool),
    Date(f64),
    Time(f64),
    Formula(String),
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "<empty>"),
            Self::Text(value) => write!(f, "{value}"),
            Self::Number(value) => write!(f, "{value}"),
            Self::Percent(value) => write!(f, "{value}%"),
            Self::Boolean(value) => write!(f, "{value}"),
            Self::Date(value) => write!(f, "{value}"),
            Self::Time(value) => write!(f, "{value}"),
            Self::Formula(value) => write!(f, "={value}"),
        }
    }
}

impl fmt::Display for ParsedEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl ParsedEntry {
    pub fn into_input(self) -> ApiResult<CellInput> {
        Ok(match self.kind {
            EntryKind::Empty => CellInput::Empty,
            EntryKind::Text(value) => CellInput::Text(value),
            EntryKind::Number(value) => CellInput::Number(value),
            EntryKind::Percent(value) => CellInput::Percent(value),
            EntryKind::Boolean(value) => CellInput::Boolean(value),
            EntryKind::Date(value) => CellInput::Date(value),
            EntryKind::Time(value) => CellInput::Time(value),
            EntryKind::Formula(value) => CellInput::formula(value)?,
        })
    }
}

/// Core error kind preserved for API consumers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiCoreErrorKind {
    InvalidSheetName(String),
    InvalidRange(String),
    InvalidSnapshot(String),
    InvalidFormula(String),
    RevisionMismatch { expected: u64, got: u64 },
    DuplicateSheetName(String),
    SheetNotFound(u32),
    CannotDeleteFinalSheet,
    InvalidAddress { sheet: u32, row: u32, column: u32 },
    ResponseForNonFormula { sheet: u32, row: u32, column: u32 },
    InvalidEngineResponse(String),
    NonFiniteNumericValue(String),
    PlanAlreadyCommitted,
    WorkbookMismatch,
    RevisionExhausted,
    Unsupported(String),
}

impl From<&CoreError> for ApiCoreErrorKind {
    fn from(value: &CoreError) -> Self {
        match value {
            CoreError::InvalidSheetName(value) => Self::InvalidSheetName(value.clone()),
            CoreError::InvalidRange(value) => Self::InvalidRange(value.clone()),
            CoreError::InvalidSnapshot(value) => Self::InvalidSnapshot(value.clone()),
            CoreError::InvalidFormula(value) => Self::InvalidFormula(value.clone()),
            CoreError::RevisionMismatch { expected, got } => Self::RevisionMismatch {
                expected: expected.0,
                got: got.0,
            },
            CoreError::DuplicateSheetName(value) => Self::DuplicateSheetName(value.clone()),
            CoreError::SheetNotFound(value) => Self::SheetNotFound(value.0),
            CoreError::CannotDeleteFinalSheet => Self::CannotDeleteFinalSheet,
            CoreError::InvalidAddress(value) => Self::InvalidAddress {
                sheet: value.sheet.0,
                row: value.row.0,
                column: value.column.0,
            },
            CoreError::ResponseForNonFormula(value) => Self::ResponseForNonFormula {
                sheet: value.sheet.0,
                row: value.row.0,
                column: value.column.0,
            },
            CoreError::InvalidEngineResponse(value) => Self::InvalidEngineResponse(value.clone()),
            CoreError::NonFiniteNumericValue(value) => Self::NonFiniteNumericValue(value.clone()),
            CoreError::PlanAlreadyCommitted => Self::PlanAlreadyCommitted,
            CoreError::WorkbookMismatch => Self::WorkbookMismatch,
            CoreError::RevisionExhausted => Self::RevisionExhausted,
            CoreError::Unsupported(value) => Self::Unsupported(value.clone()),
        }
    }
}

/// Generic application facade over any [`SpreadsheetEngine`](sheets_core::SpreadsheetEngine).
pub struct SpreadsheetApi<E>
where
    E: SpreadsheetEngine,
{
    engine: E,
    document: DocumentHandle,
    config: DocumentConfig,
    history: History,
    // Changes on content edits/replay/load, excluding navigation-only revisions.
    clipboard_epoch: u64,
}

/// Owning application handle.
#[derive(Clone)]
pub struct DocumentHandle {
    token: u64,
    workbook: Workbook,
}

impl DocumentHandle {
    pub const fn token(&self) -> u64 {
        self.token
    }

    pub fn revision(&self) -> sheets_core::Revision {
        self.workbook.revision()
    }

    pub fn active_sheet(&self) -> SheetId {
        self.workbook.active_sheet()
    }

    pub fn sheet_count(&self) -> usize {
        self.workbook.sheet_count()
    }

    pub fn snapshot(&self) -> WorkbookSnapshot {
        self.workbook.snapshot()
    }

    pub fn cell_input(&self, sheet: SheetId, address: CellAddress) -> Option<&CellInput> {
        self.workbook.cell_input(sheet, address)
    }

    pub fn cell_value(&self, sheet: SheetId, address: CellAddress) -> Option<&CellValue> {
        self.workbook.cell_value(sheet, address)
    }

    pub fn cell_format(&self, sheet: SheetId, address: CellAddress) -> Option<&CellFormat> {
        self.workbook.cell_format(sheet, address)
    }

    pub fn query_range(&self, range: &CellRange) -> Vec<CellSnapshot> {
        self.workbook.query_range(range)
    }
}

impl SpreadsheetApi<IroncalcAdapter> {
    /// Creates a production-ready API instance backed by the real IronCalc engine.
    pub fn production() -> Self {
        Self::new(IroncalcAdapter::new())
    }
}

impl<E> SpreadsheetApi<E>
where
    E: SpreadsheetEngine,
{
    /// Wraps an engine with a fresh document and default metrics.
    pub fn new(engine: E) -> Self {
        Self::with_config(engine, DocumentConfig::default())
            .expect("default document configuration must be valid")
    }

    /// Wraps an engine with explicit document metrics.
    pub fn with_config(engine: E, config: DocumentConfig) -> ApiResult<Self> {
        config.validate()?;
        let document = DocumentHandle {
            token: NEXT_DOCUMENT_TOKEN.fetch_add(1, Ordering::Relaxed),
            workbook: Workbook::new(),
        };
        Ok(Self {
            engine,
            document,
            config,
            history: History::default(),
            clipboard_epoch: 0,
        })
    }

    /// Returns the document handle.
    pub fn document(&self) -> &DocumentHandle {
        &self.document
    }

    /// Returns configured document metrics.
    pub fn config(&self) -> DocumentConfig {
        self.config.clone()
    }

    /// Executes one effective transaction against the engine and records one
    /// bounded inverse after commit. Canonical no-ops do not evaluate or change
    /// history. Navigation is excluded. Effective transactions containing sheet
    /// lifecycle or row/column structural commands form a history barrier, even
    /// when mixed with reversible edits. The previous history remains unchanged
    /// if validation, evaluation or commit fails.
    pub fn execute(&mut self, changes: ChangeSet) -> ApiResult<EngineResponse> {
        self.execute_with_history_policy(changes, false, None)
    }

    /// Sort and filter mutations require a retained inverse before evaluation.
    /// The existing prepared delta supplies the budget check without retaining
    /// another workbook snapshot. Public execute keeps RC25 barrier semantics.
    fn execute_reversible(&mut self, changes: ChangeSet) -> ApiResult<EngineResponse> {
        self.execute_with_history_policy(changes, true, None)
    }

    fn execute_with_history_policy(
        &mut self,
        changes: ChangeSet,
        require_reversible_history: bool,
        structural_owner: Option<SheetId>,
    ) -> ApiResult<EngineResponse> {
        self.execute_history_policy(
            changes,
            require_reversible_history,
            structural_owner,
            structural_owner.is_some(),
            None,
        )
    }

    fn execute_clipboard_changes(
        &mut self,
        changes: ChangeSet,
        cut: Option<(SheetId, SheetId)>,
    ) -> ApiResult<EngineResponse> {
        if !changes.changes.is_empty() {
            self.clipboard_workbook_preflight()?;
            history::preflight_clipboard_history_budget(
                &self.document.workbook,
                &changes,
                self.history.limits(),
            )?;
        }
        self.execute_history_policy(
            changes,
            true,
            cut.map(|(source, _)| source),
            true,
            cut.map(|(_, destination)| destination),
        )
    }

    fn next_clipboard_epoch(&self) -> ApiResult<u64> {
        self.clipboard_epoch.checked_add(1).ok_or_else(|| {
            ApiTransactionError::DocumentLimitExceeded(
                "clipboard mutation generation exhausted".into(),
            )
            .into()
        })
    }

    fn execute_history_policy(
        &mut self,
        changes: ChangeSet,
        require_reversible_history: bool,
        structural_owner: Option<SheetId>,
        preserve_materialized_empty: bool,
        cut_destination_owner: Option<SheetId>,
    ) -> ApiResult<EngineResponse> {
        if changes.changes.is_empty() {
            return Ok(EngineResponse::new(self.document.revision(), Vec::new()));
        }
        self.config.validate_changes(&changes)?;
        let mut plan = self
            .document
            .workbook
            .prepare(changes.clone())
            .map_err(ApiError::from)?;
        let before = self.document.snapshot();
        let after = plan.request().workbook();
        let mut recording = HistoryEntry::prepare(
            &self.document.workbook,
            &changes,
            &before,
            after,
            preserve_materialized_empty,
        )?;
        if let Recording::Entry(entry) = &mut recording
            && let Some(owner) = structural_owner
        {
            entry.mark_structural(owner);
            if let Some(destination) = cut_destination_owner {
                entry.mark_clipboard_cut_owner(destination);
            }
        }
        if matches!(recording, Recording::NoMutation) && before.active_sheet == after.active_sheet {
            return Ok(EngineResponse::new(self.document.revision(), Vec::new()));
        }
        if require_reversible_history {
            match &recording {
                Recording::Entry(entry) if !self.history.can_record_entry(entry) => {
                    return Err(ApiTransactionError::HistoryBudgetExceeded {
                        required_cell_endpoints: entry.retained_cells(),
                        required_bytes: entry.retained_bytes(),
                        limits: self.history.limits(),
                    }
                    .into());
                }
                Recording::Barrier => {
                    return Err(CoreError::Unsupported(
                        "mutation requires reversible canonical history".into(),
                    )
                    .into());
                }
                _ => {}
            }
        }
        let next_epoch = if matches!(recording, Recording::NoMutation) {
            self.clipboard_epoch
        } else {
            self.next_clipboard_epoch()?
        };
        let request = plan.engine_request();
        let response = self.engine.evaluate(request).map_err(|error| ApiError {
            kind: ApiErrorKind::Transaction(ApiTransactionError::EngineFailure(error.to_string())),
        })?;
        self.document
            .workbook
            .commit(&mut plan, response.clone())
            .map_err(ApiError::from)?;
        match recording {
            Recording::NoMutation => {}
            Recording::Entry(entry) => self.history.record(*entry),
            Recording::Barrier => self.history.barrier(),
        }
        self.clipboard_epoch = next_epoch;
        Ok(response)
    }

    /// Whether this document has a retained transaction to undo.
    pub fn can_undo(&self) -> bool {
        self.history.undo_entry().is_some()
    }

    /// Whether this document has a retained transaction to redo.
    pub fn can_redo(&self) -> bool {
        self.history.redo_entry().is_some()
    }

    /// Retained delta budgets and cursor; empty history does not imply saved.
    pub fn history_stats(&self) -> HistoryStats {
        self.history.stats()
    }

    pub fn history_limits(&self) -> HistoryLimits {
        self.history.limits()
    }

    /// Tightening limits evicts whole oldest past/furthest future transactions.
    pub fn set_history_limits(&mut self, limits: HistoryLimits) {
        self.history.set_limits(limits);
    }

    /// Replays a canonical inverse and recalculates through the same engine.
    /// No available entry is a no-op. Document identity and token are preserved.
    /// Workbook/history rollback is atomic; generic engines must keep evaluate
    /// failure-atomic themselves. Production IronCalc has no persistent state.
    pub fn undo(&mut self) -> ApiResult<EngineResponse> {
        self.replay_history(true, None)
    }

    /// Replays the next canonical forward delta with fresh recalculation.
    pub fn redo(&mut self) -> ApiResult<EngineResponse> {
        self.replay_history(false, None)
    }

    pub fn can_undo_sheet(&self, sheet: SheetId) -> bool {
        self.can_replay_sheet(true, sheet)
    }
    pub fn can_redo_sheet(&self, sheet: SheetId) -> bool {
        self.can_replay_sheet(false, sheet)
    }

    fn can_replay_sheet(&self, undo: bool, sheet: SheetId) -> bool {
        self.history
            .position(undo, Some(sheet))
            .and_then(|position| self.history.entry_at(undo, position))
            .is_some_and(|entry| entry.single_sheet() == Some(sheet))
    }

    /// Replays this sheet's latest action, bypassing independent other sheets.
    /// A matching multi-sheet transaction is rejected, never silently skipped.
    pub fn undo_sheet(&mut self, sheet: SheetId) -> ApiResult<EngineResponse> {
        self.replay_history(true, Some(sheet))
    }
    pub fn redo_sheet(&mut self, sheet: SheetId) -> ApiResult<EngineResponse> {
        self.replay_history(false, Some(sheet))
    }

    fn replay_history(&mut self, undo: bool, sheet: Option<SheetId>) -> ApiResult<EngineResponse> {
        if let Some(sheet) = sheet
            && self.document.workbook.sheet_name(sheet).is_none()
        {
            return Err(CoreError::SheetNotFound(sheet).into());
        }
        let Some(position) = self.history.position(undo, sheet) else {
            return Ok(EngineResponse::new(self.document.revision(), Vec::new()));
        };
        let entry = self
            .history
            .entry_at(undo, position)
            .expect("selected history entry exists");
        if let Some(sheet) = sheet
            && entry.single_sheet() != Some(sheet)
        {
            return Err(ApiUnsupportedError::CrossSheetHistory { sheet }.into());
        }
        if let Some(sheet) = self
            .history
            .replay_conflict(undo, position, &self.document.workbook)
        {
            return Err(ApiUnsupportedError::HistoryDependencyConflict { sheet }.into());
        }
        let changes = entry.changes(&self.document.workbook, undo);
        let next_epoch = self.next_clipboard_epoch()?;
        self.config.validate_changes(&changes)?;
        let mut plan = self.document.workbook.prepare(changes)?;
        let response = self
            .engine
            .evaluate(plan.engine_request())
            .map_err(|error| ApiTransactionError::EngineFailure(error.to_string()))?;
        self.document.workbook.commit(&mut plan, response.clone())?;
        self.clipboard_epoch = next_epoch;
        if undo {
            self.history.did_undo(position);
        } else {
            self.history.did_redo(position);
        }
        Ok(response)
    }

    /// Helper for cell mutation transactions.
    pub fn set_cell_input(
        &mut self,
        sheet: SheetId,
        address: CellAddress,
        input: CellInput,
    ) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::SetCellInput {
            sheet,
            address,
            input,
        });
        self.execute(changes)
    }

    /// Clears a cell via transaction.
    pub fn clear_cell(
        &mut self,
        sheet: SheetId,
        address: CellAddress,
    ) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::ClearCell { sheet, address });
        self.execute(changes)
    }

    /// Clears nonempty materialized inputs in an inclusive range atomically.
    /// Formats, axis dimensions and merges are preserved; empty cells are not
    /// materialized. Bounds limit the entire selected rectangle, not its inputs;
    /// `max_sheets` must allow the one target sheet.
    /// A valid selection with no inputs returns without evaluating the engine.
    pub fn clear_range(
        &mut self,
        sheet: SheetId,
        range: CellRange,
        bounds: FormatPatchBounds,
    ) -> ApiResult<EngineResponse> {
        self.validate_sheet_range(sheet, &range)?;
        if bounds.max_sheets == 0 || bounds.max_cells == 0 {
            return Err(ApiTransactionError::DocumentLimitExceeded(
                "range operations require positive cell and sheet bounds".into(),
            )
            .into());
        }
        let cell_count = range
            .rows()
            .checked_mul(range.cols())
            .ok_or_else(|| CoreError::InvalidRange("range cell count overflowed".into()))?;
        if cell_count > bounds.max_cells {
            return Err(ApiTransactionError::DocumentLimitExceeded(format!(
                "range contains {} cells, exceeding bound {}",
                cell_count, bounds.max_cells
            ))
            .into());
        }

        let mut changes = ChangeSet::new();
        for cell in self.document.query_range(&range) {
            if cell.input != CellInput::Empty {
                changes.push(Change::ClearCell {
                    sheet,
                    address: cell.address,
                });
            }
        }
        self.execute(changes)
    }

    fn validate_sheet_range(&self, sheet: SheetId, range: &CellRange) -> ApiResult<()> {
        range.validate()?;
        if sheet != range.sheet {
            return Err(CoreError::InvalidRange("cross-sheet range is not allowed".into()).into());
        }
        if self.document.workbook.sheet_name(sheet).is_none() {
            return Err(CoreError::SheetNotFound(sheet).into());
        }
        self.config.validate_range(range)
    }

    /// Adds a new sheet via transaction.
    pub fn add_sheet(&mut self, name: Option<String>) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::AddSheet { name });
        self.execute(changes)
    }

    /// Renames a sheet via transaction.
    pub fn rename_sheet(
        &mut self,
        sheet: SheetId,
        name: impl Into<String>,
    ) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::RenameSheet {
            sheet,
            name: name.into(),
        });
        self.execute(changes)
    }

    /// Deletes a sheet via transaction.
    pub fn delete_sheet(&mut self, sheet: SheetId) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::DeleteSheet { sheet });
        self.execute(changes)
    }

    /// Duplicates a sheet via transaction.
    pub fn duplicate_sheet(
        &mut self,
        sheet: SheetId,
        name: Option<String>,
    ) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::DuplicateSheet { sheet, name });
        self.execute(changes)
    }

    /// Moves a sheet within the workbook order.
    pub fn move_sheet(&mut self, sheet: SheetId, target_index: usize) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::MoveSheet {
            sheet,
            target_index,
        });
        self.execute(changes)
    }

    /// Inserts rows with native formula displacement and bounded canonical history.
    pub fn insert_rows(
        &mut self,
        sheet: SheetId,
        at: RowIndex,
        count: u32,
    ) -> ApiResult<EngineResponse> {
        self.execute_structural(StructuralOperation::InsertRows { sheet, at, count })
    }

    /// Deletes rows with native formula displacement and bounded canonical history.
    pub fn delete_rows(
        &mut self,
        sheet: SheetId,
        at: RowIndex,
        count: u32,
    ) -> ApiResult<EngineResponse> {
        self.execute_structural(StructuralOperation::DeleteRows {
            sheet,
            start: at,
            count,
        })
    }

    /// Inserts columns with native formula displacement and bounded canonical history.
    pub fn insert_columns(
        &mut self,
        sheet: SheetId,
        at: ColumnIndex,
        count: u32,
    ) -> ApiResult<EngineResponse> {
        self.execute_structural(StructuralOperation::InsertColumns { sheet, at, count })
    }

    /// Deletes columns with native formula displacement and bounded canonical history.
    pub fn delete_columns(
        &mut self,
        sheet: SheetId,
        at: ColumnIndex,
        count: u32,
    ) -> ApiResult<EngineResponse> {
        self.execute_structural(StructuralOperation::DeleteColumns {
            sheet,
            start: at,
            count,
        })
    }

    /// Sets cell format with bounds checking.
    pub fn set_cell_format(
        &mut self,
        sheet: SheetId,
        address: CellAddress,
        format: CellFormat,
        bounds: FormatPatchBounds,
    ) -> ApiResult<EngineResponse> {
        if bounds.max_sheets == 0 || bounds.max_cells == 0 {
            return Err(ApiUnsupportedError::LargeFormatMaterialization.into());
        }
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::SetCellFormat {
            sheet,
            address,
            format,
        });
        self.execute(changes)
    }

    /// Applies a sparse format patch across an inclusive range atomically.
    pub fn apply_format_range(
        &mut self,
        sheet: SheetId,
        range: CellRange,
        patch: FormatPatch,
        bounds: FormatPatchBounds,
    ) -> ApiResult<EngineResponse> {
        range.validate()?;
        if sheet != range.sheet {
            return Err(CoreError::InvalidRange("cross-sheet range is not allowed".into()).into());
        }
        let snapshot = self.document.snapshot();
        if !snapshot
            .sheets
            .iter()
            .any(|sheet_snapshot| sheet_snapshot.id == sheet)
        {
            return Err(CoreError::SheetNotFound(sheet).into());
        }
        if bounds.max_sheets == 0 || bounds.max_cells == 0 {
            return Err(ApiUnsupportedError::LargeFormatMaterialization.into());
        }
        self.config.validate_range(&range)?;

        let rows = range.rows();
        let cols = range.cols();
        let cell_count = rows
            .checked_mul(cols)
            .ok_or_else(|| CoreError::InvalidRange("range cell count overflowed".into()))?;
        if cell_count == 0 || cell_count > bounds.max_cells {
            return Err(ApiTransactionError::DocumentLimitExceeded(format!(
                "range contains {} cells, exceeding bound {}",
                cell_count, bounds.max_cells
            ))
            .into());
        }

        let validated = patch.validated()?;
        if validated.is_empty() {
            return Ok(EngineResponse::new(self.document.revision(), Vec::new()));
        }

        let mut changes = ChangeSet::new();
        for row in range.start.row.0..=range.end.row.0 {
            for col in range.start.column.0..=range.end.column.0 {
                let address = CellAddress::new(sheet, RowIndex(row), ColumnIndex(col));
                let base = self
                    .document
                    .cell_format(sheet, address)
                    .cloned()
                    .unwrap_or_default();
                let merged = validated.merge_into(&base);
                changes.push(sheets_core::Change::SetCellFormat {
                    sheet,
                    address,
                    format: merged,
                });
            }
        }

        self.execute(changes)
    }

    /// Clears cell format via transaction.
    pub fn clear_cell_format(
        &mut self,
        sheet: SheetId,
        address: CellAddress,
    ) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::ClearCellFormat { sheet, address });
        self.execute(changes)
    }

    /// Merges cells within a range.
    pub fn merge_cells(&mut self, sheet: SheetId, range: CellRange) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::MergeCells { sheet, range });
        self.execute(changes)
    }

    /// Adds a merge only when it cannot conceal inputs outside the top-left.
    /// Raw inputs count even when their calculated or displayed value is empty.
    /// Conflicting overlaps are rejected before evaluation. An identical merge
    /// is a validated no-op, including imported merges with existing inputs.
    pub fn safe_merge_cells(
        &mut self,
        sheet: SheetId,
        range: CellRange,
    ) -> ApiResult<EngineResponse> {
        self.validate_sheet_range(sheet, &range)?;
        let snapshot = self.document.snapshot();
        let sheet_snapshot = snapshot
            .sheets
            .iter()
            .find(|candidate| candidate.id == sheet)
            .ok_or(CoreError::SheetNotFound(sheet))?;
        let mut identical = false;
        for existing in &sheet_snapshot.merges {
            if existing == &range {
                identical = true;
            } else if existing.intersects(&range) {
                return Err(ApiUnsupportedError::MergeOverlapsExisting {
                    existing: existing.clone(),
                }
                .into());
            }
        }
        if identical {
            return self.execute(ChangeSet::new());
        }
        if let Some(cell) = sheet_snapshot.cells.iter().find(|cell| {
            range.contains(&cell.address)
                && cell.address != range.start
                && cell.input != CellInput::Empty
        }) {
            return Err(ApiUnsupportedError::MergeWouldConcealInput {
                address: cell.address,
            }
            .into());
        }
        self.merge_cells(sheet, range)
    }

    /// Removes the merge whose complete range exactly matches `range`.
    pub fn unmerge_cells(&mut self, sheet: SheetId, range: CellRange) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::UnmergeCells { sheet, range });
        self.execute(changes)
    }

    /// Removes every complete merge intersecting an inclusive selection in one
    /// transaction. Inputs and formatting are preserved, including portions of
    /// those merges outside the selection. No intersection is a validated no-op.
    pub fn unmerge_intersecting_selection(
        &mut self,
        sheet: SheetId,
        range: CellRange,
    ) -> ApiResult<EngineResponse> {
        self.validate_sheet_range(sheet, &range)?;
        let snapshot = self.document.snapshot();
        let sheet_snapshot = snapshot
            .sheets
            .iter()
            .find(|candidate| candidate.id == sheet)
            .ok_or(CoreError::SheetNotFound(sheet))?;
        let mut changes = ChangeSet::new();
        for existing in &sheet_snapshot.merges {
            if existing.intersects(&range) {
                changes.push(Change::UnmergeCells {
                    sheet,
                    range: existing.clone(),
                });
            }
        }
        self.execute(changes)
    }

    /// Sets active sheet via transaction.
    pub fn set_active_sheet(&mut self, sheet: SheetId) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::SetActiveSheet { sheet });
        self.execute(changes)
    }

    /// Sets row dimension via transaction.
    pub fn set_row_dimension(
        &mut self,
        sheet: SheetId,
        row: RowIndex,
        height: f64,
    ) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::SetRowDimension { sheet, row, height });
        self.execute(changes)
    }

    /// Sets column dimension via transaction.
    pub fn set_column_dimension(
        &mut self,
        sheet: SheetId,
        column: ColumnIndex,
        width: f64,
    ) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::SetColumnDimension {
            sheet,
            column,
            width,
        });
        self.execute(changes)
    }

    /// Clears row dimension via transaction.
    pub fn clear_row_dimension(
        &mut self,
        sheet: SheetId,
        row: RowIndex,
    ) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::ClearRowDimension { sheet, row });
        self.execute(changes)
    }

    /// Clears column dimension via transaction.
    pub fn clear_column_dimension(
        &mut self,
        sheet: SheetId,
        column: ColumnIndex,
    ) -> ApiResult<EngineResponse> {
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::ClearColumnDimension { sheet, column });
        self.execute(changes)
    }

    /// Ordered summaries of all sheets.
    pub fn sheet_summaries(&self) -> Vec<SheetSummary> {
        let snapshot = self.document.workbook.snapshot();
        let mut summaries = Vec::with_capacity(snapshot.sheets.len());
        for sheet in &snapshot.sheets {
            let used_cells = sheet
                .cells
                .iter()
                .filter(|cell| !matches!(cell.input, CellInput::Empty) || cell.format.is_some())
                .count() as u64;
            let merges = sheet.merges.len() as u64;
            summaries.push(SheetSummary {
                id: sheet.id,
                name: sheet.name.clone(),
                active: sheet.id == snapshot.active_sheet,
                used_cells,
                merges,
            });
        }
        summaries
    }

    /// Builds a cell view for one cell.
    pub fn cell_view(&self, sheet: SheetId, address: CellAddress) -> ApiResult<CellView> {
        if sheet != address.sheet {
            return Err(CoreError::InvalidAddress(address).into());
        }
        let input = self
            .document
            .cell_input(sheet, address)
            .cloned()
            .unwrap_or(CellInput::Empty);
        let value = self.document.cell_value(sheet, address).cloned();
        let format = self.document.cell_format(sheet, address).cloned();
        Ok(CellView::new(address, input, value, format))
    }

    /// Returns all materialized cells from the current snapshot for `sheet`.
    pub fn cell_views(&self, sheet: SheetId) -> ApiResult<Vec<CellView>> {
        let snapshot = self.document.snapshot();
        if snapshot
            .sheets
            .iter()
            .all(|sheet_snapshot| sheet_snapshot.id != sheet)
        {
            return Err(CoreError::SheetNotFound(sheet).into());
        }
        let mut views = Vec::new();
        for sheet_snapshot in snapshot.sheets.iter().filter(|s| s.id == sheet) {
            for cell in &sheet_snapshot.cells {
                let value = self.document.cell_value(sheet, cell.address).cloned();
                views.push(CellView::new(
                    cell.address,
                    cell.input.clone(),
                    value,
                    cell.format.clone(),
                ));
            }
        }
        Ok(views)
    }

    /// Returns selection summary.
    pub fn selection_summary(&self) -> SelectionSummary {
        SelectionSummary {
            sheet: self.document.workbook.active_sheet(),
            anchor: None,
            focus: None,
            range: None,
            mode: SelectionMode::None,
        }
    }

    /// Returns status summary for UI status bar.
    pub fn status_summary(&self) -> StatusSummary {
        StatusSummary {
            active_sheet: self.document.workbook.active_sheet(),
            revision: self.document.workbook.revision(),
            sheet_count: self.document.workbook.sheet_count() as u32,
        }
    }

    /// Search cells for a literal needle.
    pub fn search_literal(
        &self,
        needle: &str,
        range: Option<CellRange>,
        case_sensitive: bool,
    ) -> Vec<SearchMatch> {
        let mut matches = Vec::new();
        let ranges = range.map(|value| vec![value]).unwrap_or_else(|| {
            self.document
                .workbook
                .snapshot()
                .sheets
                .iter()
                .map(|sheet| {
                    CellRange::new(
                        CellAddress::new(sheet.id, RowIndex(0), ColumnIndex(0)),
                        CellAddress::new(sheet.id, self.config.max_row(), self.config.max_column()),
                    )
                    .expect("document range is valid")
                })
                .collect()
        });
        for range in ranges {
            for snapshot in self.document.query_range(&range) {
                let haystack = cell_input_to_raw(&snapshot.input);
                let found = if case_sensitive {
                    haystack.contains(needle)
                } else {
                    haystack.to_lowercase().contains(&needle.to_lowercase())
                };
                if found {
                    matches.push(SearchMatch {
                        sheet: range.sheet,
                        address: snapshot.address,
                        input: snapshot.input.clone(),
                        value: cell_input_into_core_value(snapshot.input.clone()),
                    });
                }
            }
        }
        matches
    }

    /// Deterministic literal search and atomic replacement.
    pub fn replace_literal(
        &mut self,
        needle: &str,
        replacement: &str,
        range: Option<CellRange>,
        case_sensitive: bool,
    ) -> ApiResult<EngineResponse> {
        if needle.is_empty() {
            return Ok(EngineResponse::new(self.document.revision(), Vec::new()));
        }
        let matches = self.search_literal(needle, range, case_sensitive);
        if matches.is_empty() {
            return Ok(EngineResponse::new(self.document.revision(), Vec::new()));
        }
        let mut changes = ChangeSet::new();
        let mut changed = false;
        for search_match in matches {
            let new_input = if case_sensitive {
                replace_in_input(&search_match.input, needle, replacement)
            } else {
                replace_in_input_case_insensitive(&search_match.input, needle, replacement)
            };
            if new_input != search_match.input {
                changed = true;
                changes.push(sheets_core::Change::SetCellInput {
                    sheet: search_match.sheet,
                    address: search_match.address,
                    input: new_input,
                });
            }
        }
        if !changed {
            return Ok(EngineResponse::new(self.document.revision(), Vec::new()));
        }
        self.execute(changes)
    }

    /// Filter rows preview without mutating state.
    pub fn filter_rows_preview(
        &self,
        range: CellRange,
        filter: fn(&CellSnapshot) -> bool,
    ) -> Vec<RowProjection> {
        let mut projections = Vec::new();
        let mut current_row: Option<(RowIndex, Vec<CellSnapshot>)> = None;
        for cell in self.document.query_range(&range) {
            match current_row {
                Some((row, mut cells)) if cell.address.row == row => {
                    cells.push(cell);
                    current_row = Some((row, cells));
                }
                Some((row, cells)) => {
                    let matches_filter = cells.iter().any(filter);
                    if matches_filter {
                        projections.push(RowProjection {
                            row,
                            normalized_cells: cells.clone(),
                        });
                    }
                    current_row = Some((cell.address.row, vec![cell]));
                }
                None => current_row = Some((cell.address.row, vec![cell])),
            }
        }
        if let Some((row, cells)) = current_row.filter(|(_, cells)| cells.iter().any(filter)) {
            projections.push(RowProjection {
                row,
                normalized_cells: cells,
            });
        }
        projections
    }

    /// Deterministic sort preview without mutating state.
    pub fn sort_preview(
        &self,
        range: CellRange,
        sort_key: fn(&CellSnapshot) -> (bool, String),
        ascending: bool,
        limit: Option<u64>,
    ) -> Vec<RowProjection> {
        let mut projections = Vec::new();
        let mut current_row: Option<(RowIndex, Vec<CellSnapshot>)> = None;
        for cell in self.document.query_range(&range) {
            match current_row {
                Some((row, mut cells)) if cell.address.row == row => {
                    cells.push(cell);
                    current_row = Some((row, cells));
                }
                Some((row, cells)) => {
                    projections.push(RowProjection {
                        row,
                        normalized_cells: cells,
                    });
                    current_row = Some((cell.address.row, vec![cell]));
                }
                None => current_row = Some((cell.address.row, vec![cell])),
            }
        }
        if let Some((row, cells)) = current_row {
            projections.push(RowProjection {
                row,
                normalized_cells: cells,
            });
        }
        projections.sort_by(|left, right| {
            let left_key = left
                .normalized_cells
                .first()
                .map_or((true, String::new()), sort_key);
            let right_key = right
                .normalized_cells
                .first()
                .map_or((true, String::new()), sort_key);
            let cmp = left_key.1.cmp(&right_key.1);
            if cmp.is_eq() {
                left_key.0.cmp(&right_key.0)
            } else {
                cmp
            }
        });
        if !ascending {
            projections.reverse();
        }
        if let Some(limit) = limit {
            projections.truncate(limit as usize);
        }
        projections
    }

    /// Applies sorted row order back into the workbook. Requires canonical row sort support.
    pub fn apply_sorted_rows(
        &mut self,
        _range: CellRange,
        _sorted_rows: &[RowProjection],
    ) -> ApiResult<EngineResponse> {
        Err(ApiUnsupportedError::SortApply.into())
    }

    /// Decodes a real XLSX package into a canonical workbook snapshot.
    pub fn import_xlsx(&self, package: &[u8]) -> ApiResult<WorkbookSnapshot> {
        let codec = self.xlsx_codec();
        let mut package = MemoryPackage::from_bytes(package.to_vec());
        codec.import(&mut package).map_err(|error| {
            ApiTransactionError::EngineFailure(format!("XLSX import failed: {error}")).into()
        })
    }

    /// Opens XLSX data as the live document and recalculates formulas through
    /// the configured engine before the replacement becomes visible.
    ///
    /// The previous document stays unchanged when parsing, validation, or
    /// recalculation fails.
    pub fn load_xlsx(&mut self, package: &[u8]) -> ApiResult<WorkbookSnapshot> {
        let next_epoch = self.next_clipboard_epoch()?;
        let bounded_import = uses_bounded_import(package);
        let snapshot = self.import_xlsx(package)?;
        self.config.validate_snapshot(&snapshot)?;
        let mut workbook = Workbook::from_snapshot(snapshot).map_err(ApiError::from)?;

        if !bounded_import {
            let mut plan = workbook.prepare(ChangeSet::new()).map_err(ApiError::from)?;
            let response = self
                .engine
                .evaluate(plan.engine_request())
                .map_err(|error| ApiError {
                    kind: ApiErrorKind::Transaction(ApiTransactionError::EngineFailure(
                        error.to_string(),
                    )),
                })?;
            workbook
                .commit(&mut plan, response)
                .map_err(ApiError::from)?;
        }

        let loaded = workbook.snapshot();
        self.document.workbook = workbook;
        self.clipboard_epoch = next_epoch;
        self.history.checkpoint();
        Ok(loaded)
    }

    /// Compatibility name for [`SpreadsheetApi::import_xlsx`].
    pub fn import_snapshot(&self, package: &[u8]) -> ApiResult<WorkbookSnapshot> {
        self.import_xlsx(package)
    }

    /// Encodes the current workbook as a real XLSX ZIP/XML package.
    pub fn export_xlsx(&self) -> ApiResult<Vec<u8>> {
        let snapshot = self.document.snapshot();
        let mut package = MemoryPackage::new();
        self.xlsx_codec()
            .export(&snapshot, &mut package)
            .map(|_| package.into_bytes())
            .map_err(|error| {
                ApiTransactionError::EngineFailure(format!("XLSX export failed: {error}")).into()
            })
    }

    /// Compatibility name for [`SpreadsheetApi::export_xlsx`].
    pub fn export_snapshot(&self) -> ApiResult<Vec<u8>> {
        self.export_xlsx()
    }

    /// Returns the production XLSX codec backed by local IronCalc sources.
    pub fn xlsx_codec(&self) -> XlsxCodec<sheets_xlsx::IroncalcXlsxParts> {
        ironcalc_codec()
    }
}

/// Application status summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusSummary {
    pub active_sheet: SheetId,
    pub revision: sheets_core::Revision,
    pub sheet_count: u32,
}

impl fmt::Display for StatusSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "revision={} sheets={} active={}",
            self.revision, self.sheet_count, self.active_sheet
        )
    }
}

impl fmt::Display for SheetSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} id={} cells={} merges={}",
            self.name, self.id, self.used_cells, self.merges
        )
    }
}

fn cell_input_to_raw(input: &CellInput) -> String {
    match input {
        CellInput::Empty => String::new(),
        CellInput::Text(value) => value.clone(),
        CellInput::Formula(value) => format!("={value}"),
        CellInput::Number(value) => value.to_string(),
        CellInput::Percent(value) => value.to_string(),
        CellInput::Date(value) => value.to_string(),
        CellInput::Time(value) => value.to_string(),
        CellInput::Boolean(value) => value.to_string(),
    }
}

fn cell_input_to_display(input: &CellInput, value: &Option<CellValue>) -> String {
    match value {
        Some(CellValue::Text(v)) => v.clone(),
        Some(CellValue::Number(v)) => v.to_string(),
        Some(CellValue::Percent(v)) => format!("{v}%"),
        Some(CellValue::Boolean(v)) => v.to_string(),
        Some(CellValue::Date(v)) => v.to_string(),
        Some(CellValue::Time(v)) => v.to_string(),
        Some(CellValue::Error(e)) => e.to_string(),
        Some(CellValue::Empty) => String::new(),
        None => cell_input_to_raw(input),
    }
}

fn replace_in_input(input: &CellInput, needle: &str, replacement: &str) -> CellInput {
    match input {
        CellInput::Text(value) => CellInput::Text(value.replace(needle, replacement)),
        CellInput::Formula(value) => CellInput::Formula(value.replace(needle, replacement)),
        _ => input.clone(),
    }
}

fn replace_in_input_case_insensitive(
    input: &CellInput,
    needle: &str,
    replacement: &str,
) -> CellInput {
    fn find_all_case_insensitive(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
        if needle.is_empty() {
            return Vec::new();
        }
        let needle_chars: Vec<char> = needle.chars().collect();
        let needle_len = needle_chars.len();
        let haystack_chars: Vec<char> = haystack.chars().collect();
        let haystack_len = haystack_chars.len();
        let mut matches = Vec::new();
        let mut i = 0;
        while i + needle_len <= haystack_len {
            let haystack_sub: String = haystack_chars[i..i + needle_len].iter().collect();
            if haystack_sub.eq_ignore_ascii_case(needle) {
                let byte_start = haystack_chars[..i].iter().map(|c| c.len_utf8()).sum();
                let byte_end = byte_start
                    + haystack_chars[i..i + needle_len]
                        .iter()
                        .map(|c| c.len_utf8())
                        .sum::<usize>();
                matches.push((byte_start, byte_end));
                i += needle_len;
            } else {
                i += 1;
            }
        }
        matches
    }

    match input {
        CellInput::Text(value) => {
            let ranges = find_all_case_insensitive(value, needle);
            if ranges.is_empty() {
                return input.clone();
            }
            let mut result = String::with_capacity(value.len());
            let mut last = 0;
            for (start, end) in ranges {
                result.push_str(&value[last..start]);
                result.push_str(replacement);
                last = end;
            }
            result.push_str(&value[last..]);
            CellInput::Text(result)
        }
        CellInput::Formula(value) => {
            let ranges = find_all_case_insensitive(value, needle);
            if ranges.is_empty() {
                return input.clone();
            }
            let mut result = String::with_capacity(value.len());
            let mut last = 0;
            for (start, end) in ranges {
                result.push_str(&value[last..start]);
                result.push_str(replacement);
                last = end;
            }
            result.push_str(&value[last..]);
            CellInput::Formula(result)
        }
        _ => input.clone(),
    }
}

fn cell_input_into_core_value(input: CellInput) -> Option<CellValue> {
    Some(match input {
        CellInput::Empty => CellValue::Empty,
        CellInput::Text(value) => CellValue::Text(value),
        CellInput::Number(value) => CellValue::Number(value),
        CellInput::Percent(value) => CellValue::Percent(value),
        CellInput::Date(value) => CellValue::Date(value),
        CellInput::Time(value) => CellValue::Time(value),
        CellInput::Boolean(value) => CellValue::Boolean(value),
        CellInput::Formula(_) => CellValue::Empty,
    })
}

/// Shared document parsing helpers.
pub struct EntryParser;

impl EntryParser {
    pub fn parse(raw: &str) -> ApiResult<ParsedEntry> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(ParsedEntry {
                kind: EntryKind::Empty,
                raw: String::new(),
            });
        }
        Self::parse_non_empty(trimmed)
    }

    fn parse_non_empty(trimmed: &str) -> ApiResult<ParsedEntry> {
        if let Some(formula) = Self::parse_formula(trimmed) {
            return formula;
        }
        if let Some(boolean) = Self::parse_boolean(trimmed) {
            return boolean;
        }
        if let Some(percent) = Self::parse_percent(trimmed) {
            return percent;
        }
        if let Some(date) = Self::parse_date(trimmed) {
            return date;
        }
        if let Some(time) = Self::parse_time(trimmed) {
            return time;
        }
        if let Some(number) = Self::parse_number(trimmed) {
            return number;
        }
        Ok(ParsedEntry {
            kind: EntryKind::Text(trimmed.into()),
            raw: trimmed.into(),
        })
    }

    fn parse_formula(trimmed: &str) -> Option<ApiResult<ParsedEntry>> {
        if !trimmed.starts_with('=') {
            return None;
        }
        let normalized = trimmed[1..].trim();
        if normalized.is_empty() {
            return Some(Err(ApiParseError::InvalidFormula(
                "formula cannot be empty".into(),
            )
            .into()));
        }
        Some(Ok(ParsedEntry {
            kind: EntryKind::Formula(normalized.into()),
            raw: trimmed.into(),
        }))
    }

    fn parse_boolean(trimmed: &str) -> Option<ApiResult<ParsedEntry>> {
        match trimmed {
            "TRUE" | "True" | "true" => Some(Ok(ParsedEntry {
                kind: EntryKind::Boolean(true),
                raw: trimmed.into(),
            })),
            "FALSE" | "False" | "false" => Some(Ok(ParsedEntry {
                kind: EntryKind::Boolean(false),
                raw: trimmed.into(),
            })),
            _ => None,
        }
    }

    fn parse_percent(trimmed: &str) -> Option<ApiResult<ParsedEntry>> {
        if !trimmed.ends_with('%') || trimmed.len() == 1 {
            return None;
        }
        let inner = &trimmed[..trimmed.len() - 1];
        let value = inner.parse::<f64>().ok()?;
        if !value.is_finite() {
            return Some(Err(ApiParseError::InvalidNumber(trimmed.into()).into()));
        }
        Some(Ok(ParsedEntry {
            kind: EntryKind::Percent(value / 100.0),
            raw: trimmed.into(),
        }))
    }

    fn parse_date(trimmed: &str) -> Option<ApiResult<ParsedEntry>> {
        let parts: Vec<&str> = trimmed.split('-').collect();
        if parts.len() != 3 {
            return None;
        }
        let year = parts[0].parse::<f64>().ok()?;
        let month = parts[1].parse::<f64>().ok()?;
        let day = parts[2].parse::<f64>().ok()?;
        if !year.is_finite() || !month.is_finite() || !day.is_finite() {
            return Some(Err(ApiParseError::InvalidDate(trimmed.into()).into()));
        }
        if !(1.0..=12.0).contains(&month) || !(1.0..=31.0).contains(&day) {
            return Some(Err(ApiParseError::InvalidDate(trimmed.into()).into()));
        }
        Some(Ok(ParsedEntry {
            kind: EntryKind::Date(year * 10000.0 + month * 100.0 + day),
            raw: trimmed.into(),
        }))
    }

    fn parse_time(trimmed: &str) -> Option<ApiResult<ParsedEntry>> {
        let parts: Vec<&str> = trimmed.split(':').collect();
        if parts.len() != 2 && parts.len() != 3 {
            return None;
        }
        let hours = parts[0].parse::<f64>().ok()?;
        let minutes = parts[1].parse::<f64>().ok()?;
        let seconds = if parts.len() == 3 {
            parts[2].parse::<f64>().ok()?
        } else {
            0.0
        };
        if !hours.is_finite() || !minutes.is_finite() || !seconds.is_finite() {
            return Some(Err(ApiParseError::InvalidTime(trimmed.into()).into()));
        }
        if !(0.0..60.0).contains(&minutes) || !(0.0..60.0).contains(&seconds) {
            return Some(Err(ApiParseError::InvalidTime(trimmed.into()).into()));
        }
        let value = hours / 24.0 + minutes / 1440.0 + seconds / 86400.0;
        Some(Ok(ParsedEntry {
            kind: EntryKind::Time(value),
            raw: trimmed.into(),
        }))
    }

    fn parse_number(trimmed: &str) -> Option<ApiResult<ParsedEntry>> {
        let value = trimmed.parse::<f64>().ok()?;
        if !value.is_finite() {
            return Some(Err(ApiParseError::InvalidNumber(trimmed.into()).into()));
        }
        Some(Ok(ParsedEntry {
            kind: EntryKind::Number(value),
            raw: trimmed.into(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use sheets_core::{CellAddress, CellInput, ColumnIndex, EngineResponse, Revision, RowIndex};

    #[derive(Debug, Default)]
    struct FakeEngine;

    impl SpreadsheetEngine for FakeEngine {
        type Error = CoreError;

        fn evaluate(
            &mut self,
            request: sheets_core::EngineRequest,
        ) -> Result<EngineResponse, Self::Error> {
            Ok(EngineResponse::new(request.base_revision(), Vec::new()))
        }
    }

    #[derive(Debug, Default)]
    struct RangeOperationsEngine {
        calls: usize,
        fail: bool,
    }

    impl SpreadsheetEngine for RangeOperationsEngine {
        type Error = CoreError;

        fn evaluate(
            &mut self,
            request: sheets_core::EngineRequest,
        ) -> Result<EngineResponse, Self::Error> {
            self.calls += 1;
            if self.fail {
                return Err(CoreError::Unsupported("test engine failure".into()));
            }
            Ok(EngineResponse::new(request.base_revision(), Vec::new()))
        }
    }

    fn range_operations_address(sheet: SheetId, row: u32, column: u32) -> CellAddress {
        CellAddress::new(sheet, RowIndex(row), ColumnIndex(column))
    }

    fn range_operations_range(sheet: SheetId, start: (u32, u32), end: (u32, u32)) -> CellRange {
        CellRange::new(
            range_operations_address(sheet, start.0, start.1),
            range_operations_address(sheet, end.0, end.1),
        )
        .unwrap()
    }

    fn range_operations_bounds(max_cells: u64) -> FormatPatchBounds {
        FormatPatchBounds {
            max_cells,
            max_sheets: 1,
        }
    }

    #[test]
    fn range_operations_clear_is_sparse_atomic_and_preserves_metadata() {
        let mut api = SpreadsheetApi::new(RangeOperationsEngine::default());
        let sheet = api.document.active_sheet();
        let formatted = range_operations_address(sheet, 0, 0);
        let format_only = range_operations_address(sheet, 1, 1);
        let unformatted = range_operations_address(sheet, 2, 2);
        let outside = range_operations_address(sheet, 100, 0);
        let format = CellFormat {
            font: sheets_core::Font {
                bold: true,
                ..sheets_core::Font::default()
            },
            ..CellFormat::default()
        };
        api.set_cell_input(sheet, formatted, CellInput::Number(8.0))
            .unwrap();
        api.set_cell_input(sheet, unformatted, CellInput::Text("clear me".into()))
            .unwrap();
        api.set_cell_input(sheet, outside, CellInput::Number(9.0))
            .unwrap();
        for address in [formatted, format_only] {
            api.set_cell_format(sheet, address, format.clone(), range_operations_bounds(1))
                .unwrap();
        }
        api.set_row_dimension(sheet, RowIndex(0), 40.0).unwrap();
        api.set_column_dimension(sheet, ColumnIndex(0), 120.0)
            .unwrap();
        api.merge_cells(sheet, range_operations_range(sheet, (0, 0), (1, 1)))
            .unwrap();
        api.add_sheet(Some("Untouched".into())).unwrap();
        let other = api.document.active_sheet();
        api.set_cell_input(
            other,
            range_operations_address(other, 0, 0),
            CellInput::Number(7.0),
        )
        .unwrap();
        let before = api.document.snapshot();
        let calls = api.engine.calls;
        let selection = range_operations_range(sheet, (0, 0), (99, 99));

        let response = api
            .clear_range(sheet, selection.clone(), range_operations_bounds(10_000))
            .unwrap();

        assert_eq!(response.base_revision(), before.revision);
        assert_eq!(
            api.document.revision(),
            before.revision.checked_next().unwrap()
        );
        assert_eq!(api.engine.calls, calls + 1);
        assert_eq!(
            api.document.cell_input(sheet, formatted),
            Some(&CellInput::Empty)
        );
        assert_eq!(api.document.cell_format(sheet, formatted), Some(&format));
        assert_eq!(api.document.cell_format(sheet, format_only), Some(&format));
        assert_eq!(api.document.cell_input(sheet, unformatted), None);
        assert_eq!(
            api.document.cell_input(sheet, outside),
            Some(&CellInput::Number(9.0))
        );
        let after = api.document.snapshot();
        assert_eq!(after.sheets[0].cells.len(), 3);
        assert_eq!(after.sheets[0].merges, before.sheets[0].merges);
        assert_eq!(
            after.sheets[0].row_dimensions,
            before.sheets[0].row_dimensions
        );
        assert_eq!(
            after.sheets[0].column_dimensions,
            before.sheets[0].column_dimensions
        );
        assert_eq!(after.sheets[1], before.sheets[1]);
        let no_op = api
            .clear_range(sheet, selection, range_operations_bounds(10_000))
            .unwrap();
        assert_eq!(no_op.base_revision(), after.revision);
        assert_eq!(api.engine.calls, calls + 1);
        assert_eq!(api.document.snapshot(), after);
    }

    #[test]
    fn range_operations_clear_recalculates_dependents_with_ironcalc() {
        let mut api = SpreadsheetApi::production();
        let sheet = api.document.active_sheet();
        let a1 = range_operations_address(sheet, 0, 0);
        let b1 = range_operations_address(sheet, 0, 1);
        api.set_cell_input(sheet, a1, CellInput::Number(4.0))
            .unwrap();
        api.set_cell_input(sheet, b1, CellInput::formula("=A1+1").unwrap())
            .unwrap();
        assert_eq!(
            api.document.cell_value(sheet, b1),
            Some(&CellValue::Number(5.0))
        );
        let revision = api.document.revision();
        api.clear_range(
            sheet,
            range_operations_range(sheet, (0, 0), (0, 0)),
            range_operations_bounds(1),
        )
        .unwrap();
        assert_eq!(
            api.document.cell_value(sheet, b1),
            Some(&CellValue::Number(1.0))
        );
        assert_eq!(api.document.cell_input(sheet, a1), None);
        assert_eq!(api.document.revision(), revision.checked_next().unwrap());
    }

    #[test]
    fn range_operations_clear_validates_before_any_transaction() {
        let mut api = SpreadsheetApi::with_config(
            RangeOperationsEngine::default(),
            DocumentConfig::new(2, 2, 25, 90),
        )
        .unwrap();
        let sheet = api.document.active_sheet();
        api.set_cell_input(
            sheet,
            range_operations_address(sheet, 0, 0),
            CellInput::Number(1.0),
        )
        .unwrap();
        let valid = range_operations_range(sheet, (0, 0), (1, 1));
        let mut malformed = valid.clone();
        malformed.end.sheet = SheetId(99);
        let mut reversed = valid.clone();
        reversed.start.row = RowIndex(1);
        reversed.end.row = RowIndex(0);
        let unknown = SheetId(99);
        let cases = [
            (
                unknown,
                range_operations_range(unknown, (0, 0), (0, 0)),
                range_operations_bounds(4),
            ),
            (unknown, valid.clone(), range_operations_bounds(4)),
            (sheet, malformed, range_operations_bounds(4)),
            (sheet, reversed, range_operations_bounds(4)),
            (
                sheet,
                range_operations_range(sheet, (0, 0), (2, 0)),
                range_operations_bounds(4),
            ),
            (
                sheet,
                range_operations_range(sheet, (0, 0), (0, 2)),
                range_operations_bounds(4),
            ),
            (sheet, valid.clone(), range_operations_bounds(3)),
            (sheet, valid.clone(), range_operations_bounds(0)),
            (
                sheet,
                valid,
                FormatPatchBounds {
                    max_cells: 4,
                    max_sheets: 0,
                },
            ),
        ];
        let before = api.document.snapshot();
        let calls = api.engine.calls;
        for (target, range, bounds) in cases {
            assert!(api.clear_range(target, range, bounds).is_err());
            assert_eq!(api.document.snapshot(), before);
            assert_eq!(api.engine.calls, calls);
        }
        // A sparse or completely empty rectangle still has to fit the bound.
        assert!(matches!(
            api.clear_range(
                sheet,
                range_operations_range(sheet, (0, 0), (1, 1)),
                range_operations_bounds(3)
            )
            .unwrap_err()
            .kind(),
            ApiErrorKind::Transaction(ApiTransactionError::DocumentLimitExceeded(_))
        ));
    }

    #[test]
    fn range_operations_safe_merge_preserves_anchor_and_formats_with_ironcalc() {
        let mut api = SpreadsheetApi::production();
        let sheet = api.document.active_sheet();
        let a1 = range_operations_address(sheet, 0, 0);
        let b2 = range_operations_address(sheet, 1, 1);
        let formula = CellInput::formula("=1+2").unwrap();
        api.set_cell_input(sheet, a1, formula.clone()).unwrap();
        let format = CellFormat::default();
        api.set_cell_format(sheet, b2, format.clone(), range_operations_bounds(1))
            .unwrap();
        let before = api.document.snapshot();
        let range = range_operations_range(sheet, (0, 0), (1, 1));
        let response = api.safe_merge_cells(sheet, range.clone()).unwrap();
        assert_eq!(response.base_revision(), before.revision);
        assert_eq!(
            api.document.revision(),
            before.revision.checked_next().unwrap()
        );
        assert_eq!(api.document.cell_input(sheet, a1), Some(&formula));
        assert_eq!(
            api.document.cell_value(sheet, a1),
            Some(&CellValue::Number(3.0))
        );
        assert_eq!(api.document.cell_format(sheet, b2), Some(&format));
        let after = api.document.snapshot();
        assert_eq!(after.sheets[0].cells, before.sheets[0].cells);
        assert_eq!(after.sheets[0].merges, vec![range.clone()]);
        api.safe_merge_cells(sheet, range).unwrap();
        assert_eq!(api.document.snapshot(), after);
    }

    #[test]
    fn range_operations_safe_merge_rejects_all_non_anchor_inputs() {
        let inputs = [
            CellInput::Number(0.0),
            CellInput::Boolean(false),
            CellInput::Text(String::new()),
            CellInput::formula("=\"\"").unwrap(),
            CellInput::Percent(0.0),
            CellInput::Date(1.0),
            CellInput::Time(0.0),
        ];
        for input in inputs {
            let mut api = SpreadsheetApi::new(RangeOperationsEngine::default());
            let sheet = api.document.active_sheet();
            let non_anchor = range_operations_address(sheet, 0, 1);
            api.set_cell_input(sheet, non_anchor, input).unwrap();
            let before = api.document.snapshot();
            let calls = api.engine.calls;
            let error = api
                .safe_merge_cells(sheet, range_operations_range(sheet, (0, 0), (1, 1)))
                .unwrap_err();
            assert!(matches!(
                error.kind(),
                ApiErrorKind::Unsupported(ApiUnsupportedError::MergeWouldConcealInput { address }) if *address == non_anchor
            ));
            assert!(error.to_string().contains("row 1, column 2"));
            assert_eq!(api.document.snapshot(), before);
            assert_eq!(api.engine.calls, calls);
        }
    }

    #[test]
    fn range_operations_safe_merge_rejects_overlap_and_identical_is_noop() {
        let mut api = SpreadsheetApi::new(RangeOperationsEngine::default());
        let sheet = api.document.active_sheet();
        let existing = range_operations_range(sheet, (1, 1), (2, 2));
        api.merge_cells(sheet, existing.clone()).unwrap();
        // Existing imported/legacy merges can already contain concealed inputs.
        api.set_cell_input(
            sheet,
            range_operations_address(sheet, 2, 2),
            CellInput::Number(5.0),
        )
        .unwrap();
        let before = api.document.snapshot();
        let calls = api.engine.calls;
        let response = api.safe_merge_cells(sheet, existing.clone()).unwrap();
        assert_eq!(response.base_revision(), before.revision);
        assert_eq!(api.document.snapshot(), before);
        assert_eq!(api.engine.calls, calls);
        for range in [
            range_operations_range(sheet, (0, 0), (1, 1)),
            range_operations_range(sheet, (0, 0), (3, 3)),
            range_operations_range(sheet, (2, 2), (2, 2)),
        ] {
            let error = api.safe_merge_cells(sheet, range).unwrap_err();
            assert!(matches!(
                error.kind(),
                ApiErrorKind::Unsupported(ApiUnsupportedError::MergeOverlapsExisting { existing: conflict }) if conflict == &existing
            ));
            assert_eq!(api.document.snapshot(), before);
            assert_eq!(api.engine.calls, calls);
        }
        api.safe_merge_cells(sheet, range_operations_range(sheet, (3, 3), (4, 4)))
            .unwrap();
        assert_eq!(api.engine.calls, calls + 1);
    }

    #[test]
    fn range_operations_unmerge_removes_complete_intersections_once() {
        let mut api = SpreadsheetApi::new(RangeOperationsEngine::default());
        let sheet = api.document.active_sheet();
        let first = range_operations_range(sheet, (0, 0), (1, 1));
        let second = range_operations_range(sheet, (2, 2), (3, 3));
        let distant = range_operations_range(sheet, (5, 5), (6, 6));
        for range in [&first, &second, &distant] {
            api.merge_cells(sheet, range.clone()).unwrap();
        }
        let a1 = range_operations_address(sheet, 0, 0);
        let d4 = range_operations_address(sheet, 3, 3);
        api.set_cell_input(sheet, a1, CellInput::Number(2.0))
            .unwrap();
        api.set_cell_input(sheet, d4, CellInput::Text("retain concealed input".into()))
            .unwrap();
        api.set_cell_format(sheet, d4, CellFormat::default(), range_operations_bounds(1))
            .unwrap();
        api.set_row_dimension(sheet, RowIndex(3), 60.0).unwrap();
        api.add_sheet(Some("Other".into())).unwrap();
        let other = api.document.active_sheet();
        api.merge_cells(other, range_operations_range(other, (0, 0), (1, 1)))
            .unwrap();
        let selection = range_operations_range(sheet, (1, 1), (2, 2));
        let before = api.document.snapshot();
        let calls = api.engine.calls;
        // The original method requires exact equality, not intersection.
        assert!(api.unmerge_cells(sheet, selection.clone()).is_err());
        assert_eq!(api.document.snapshot(), before);
        assert_eq!(api.engine.calls, calls);

        let response = api
            .unmerge_intersecting_selection(sheet, selection.clone())
            .unwrap();
        assert_eq!(response.base_revision(), before.revision);
        assert_eq!(
            api.document.revision(),
            before.revision.checked_next().unwrap()
        );
        assert_eq!(api.engine.calls, calls + 1);
        let after = api.document.snapshot();
        assert_eq!(after.sheets[0].merges, vec![distant]);
        assert_eq!(after.sheets[0].cells, before.sheets[0].cells);
        assert_eq!(
            after.sheets[0].row_dimensions,
            before.sheets[0].row_dimensions
        );
        assert_eq!(
            after.sheets[0].column_dimensions,
            before.sheets[0].column_dimensions
        );
        assert_eq!(after.sheets[1], before.sheets[1]);
        api.unmerge_intersecting_selection(sheet, selection)
            .unwrap();
        assert_eq!(api.document.snapshot(), after);
        assert_eq!(api.engine.calls, calls + 1);
    }

    #[test]
    fn range_operations_merge_and_unmerge_validate_even_for_noops() {
        let mut api = SpreadsheetApi::with_config(
            RangeOperationsEngine::default(),
            DocumentConfig::new(2, 2, 25, 90),
        )
        .unwrap();
        let sheet = api.document.active_sheet();
        let unknown = SheetId(99);
        let mut malformed = range_operations_range(sheet, (0, 0), (1, 1));
        malformed.sheet = unknown;
        let cases = [
            (unknown, range_operations_range(unknown, (0, 0), (0, 0))),
            (unknown, range_operations_range(sheet, (0, 0), (0, 0))),
            (sheet, malformed),
            (sheet, range_operations_range(sheet, (0, 0), (2, 0))),
            (sheet, range_operations_range(sheet, (0, 0), (0, 2))),
        ];
        let before = api.document.snapshot();
        for (target, range) in cases {
            assert!(api.safe_merge_cells(target, range.clone()).is_err());
            assert!(api.unmerge_intersecting_selection(target, range).is_err());
            assert_eq!(api.document.snapshot(), before);
            assert_eq!(api.engine.calls, 0);
        }
        api.unmerge_intersecting_selection(sheet, range_operations_range(sheet, (0, 0), (1, 1)))
            .unwrap();
        assert_eq!(api.document.snapshot(), before);
        assert_eq!(api.engine.calls, 0);
    }

    #[test]
    fn range_operations_engine_failure_rolls_back_all_three_operations() {
        let mut api = SpreadsheetApi::new(RangeOperationsEngine::default());
        let sheet = api.document.active_sheet();
        api.set_cell_input(
            sheet,
            range_operations_address(sheet, 0, 0),
            CellInput::Number(1.0),
        )
        .unwrap();
        api.merge_cells(sheet, range_operations_range(sheet, (1, 1), (2, 2)))
            .unwrap();
        let before = api.document.snapshot();
        let calls = api.engine.calls;
        api.engine.fail = true;
        let results = [
            api.clear_range(
                sheet,
                range_operations_range(sheet, (0, 0), (0, 0)),
                range_operations_bounds(1),
            ),
            api.safe_merge_cells(sheet, range_operations_range(sheet, (4, 4), (5, 5))),
            api.unmerge_intersecting_selection(
                sheet,
                range_operations_range(sheet, (2, 2), (2, 2)),
            ),
        ];
        for result in results {
            assert!(matches!(
                result.unwrap_err().kind(),
                ApiErrorKind::Transaction(ApiTransactionError::EngineFailure(_))
            ));
        }
        assert_eq!(api.document.snapshot(), before);
        assert_eq!(api.engine.calls, calls + 3);
    }

    #[test]
    fn create_api_with_defaults() {
        let api = SpreadsheetApi::new(FakeEngine);
        assert_eq!(api.config().max_document_rows, 1_048_576);
        assert_eq!(api.config().max_document_columns, 16_384);
        let metrics = default_grid_metrics();
        assert_eq!(
            api.config().default_row_height,
            metrics.row_height.round() as u16
        );
        assert_eq!(
            api.config().default_column_width,
            metrics.column_width.round() as u16
        );
    }

    #[test]
    fn production_constructor_creates_api() {
        let api = SpreadsheetApi::<IroncalcAdapter>::production();
        assert_eq!(api.config().max_document_rows, 1_048_576);
        assert_eq!(api.document().sheet_count(), 1);
    }

    #[test]
    fn configured_bounds_reject_invalid_documents_and_changes() {
        let invalid =
            match SpreadsheetApi::with_config(FakeEngine, DocumentConfig::new(0, 2, 24, 96)) {
                Ok(_) => panic!("zero rows must be rejected"),
                Err(error) => error,
            };
        assert!(matches!(
            invalid.kind(),
            ApiErrorKind::Transaction(ApiTransactionError::InvalidDocumentConfig(_))
        ));

        let mut api = SpreadsheetApi::with_config(FakeEngine, DocumentConfig::new(2, 2, 24, 96))
            .expect("valid document configuration");
        let sheet = api.document.active_sheet();
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::SetCellInput {
            sheet,
            address: CellAddress::new(sheet, RowIndex(2), ColumnIndex(0)),
            input: CellInput::Number(1.0),
        });
        let error = match api.execute(changes) {
            Ok(_) => panic!("out-of-bounds change must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(
            error.kind(),
            ApiErrorKind::Transaction(ApiTransactionError::DocumentLimitExceeded(_))
        ));
    }

    #[test]
    fn transaction_returns_engine_response() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let result = api
            .add_sheet(Some("Data".into()))
            .expect("sheet add transaction succeeds");
        assert_eq!(result.base_revision(), Revision(0));
        assert_eq!(api.document.sheet_count(), 2);
    }

    #[test]
    fn stale_revision_is_rejected() {
        let mut wb = Workbook::new();
        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::AddSheet {
            name: Some("Old".into()),
        });
        let mut plan = wb.prepare(changes).unwrap();
        wb.commit(&mut plan, EngineResponse::new(Revision(0), Vec::new()))
            .unwrap();

        let mut changes = ChangeSet::new();
        changes.push(sheets_core::Change::AddSheet {
            name: Some("New".into()),
        });
        let mut plan2 = wb.prepare(changes).unwrap();
        let result = wb.commit(&mut plan2, EngineResponse::new(Revision(0), Vec::new()));
        assert!(result.is_err());
    }

    #[test]
    fn entry_parser_handles_formula_and_literals() {
        assert_eq!(EntryParser::parse("").unwrap().kind, EntryKind::Empty);
        assert_eq!(
            EntryParser::parse("=SUM(A1)").unwrap().kind,
            EntryKind::Formula("SUM(A1)".into())
        );
        assert_eq!(
            EntryParser::parse("42").unwrap().kind,
            EntryKind::Number(42.0)
        );
        assert_eq!(
            EntryParser::parse("42%").unwrap().kind,
            EntryKind::Percent(0.42)
        );
        assert_eq!(
            EntryParser::parse("TRUE").unwrap().kind,
            EntryKind::Boolean(true)
        );
        assert_eq!(
            EntryParser::parse("2024-12-31").unwrap().kind,
            EntryKind::Date(20241231.0)
        );
        assert_eq!(
            EntryParser::parse("23:59:00").unwrap().kind,
            EntryKind::Time(0.9993055555555556)
        );
        assert_eq!(
            EntryParser::parse("plain text").unwrap().kind,
            EntryKind::Text("plain text".into())
        );
    }

    #[test]
    fn search_matches_are_deterministic() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        api.set_cell_input(sheet, address, CellInput::Text("hello world".into()))
            .unwrap();
        let matches = api.search_literal("world", None, true);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].input, CellInput::Text("hello world".into()));
    }

    #[test]
    fn format_patch_bounds_reject_zero_limits() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let result = api.set_cell_format(
            sheet,
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(0)),
            CellFormat::default(),
            FormatPatchBounds::default(),
        );
        assert!(matches!(
            result.unwrap_err().kind(),
            ApiErrorKind::Unsupported(_)
        ));
    }

    #[test]
    fn filter_rows_preview_multi_row_regression() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        api.set_cell_input(
            sheet,
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(0)),
            CellInput::Text("keep".into()),
        )
        .unwrap();
        api.set_cell_input(
            sheet,
            CellAddress::new(sheet, RowIndex(1), ColumnIndex(0)),
            CellInput::Text("drop".into()),
        )
        .unwrap();
        api.set_cell_input(
            sheet,
            CellAddress::new(sheet, RowIndex(2), ColumnIndex(0)),
            CellInput::Text("keep".into()),
        )
        .unwrap();
        let range = CellRange::new(
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(0)),
            CellAddress::new(sheet, RowIndex(2), ColumnIndex(0)),
        )
        .unwrap();
        let projections = api.filter_rows_preview(
            range,
            |cell| matches!(cell.input, CellInput::Text(ref v) if v == "keep"),
        );
        assert_eq!(projections.len(), 2);
        assert_eq!(projections[0].row, RowIndex(0));
        assert_eq!(projections[0].normalized_cells.len(), 1);
        assert_eq!(projections[1].row, RowIndex(2));
        assert_eq!(projections[1].normalized_cells.len(), 1);
    }

    #[test]
    fn cell_view_validates_sheet_match() {
        let api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let other_sheet = {
            let mut wb = Workbook::new();
            let mut changes = ChangeSet::new();
            changes.push(sheets_core::Change::AddSheet {
                name: Some("Other".into()),
            });
            let mut plan = wb.prepare(changes).unwrap();
            wb.commit(&mut plan, EngineResponse::new(Revision(0), Vec::new()))
                .unwrap();
            wb.active_sheet()
        };
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        let result = api.cell_view(other_sheet, address);
        assert!(matches!(
            result.unwrap_err().kind(),
            ApiErrorKind::Core(CoreError::InvalidAddress(_))
        ));
    }

    #[test]
    fn cell_views_returns_snapshot_ordered_views() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        api.set_cell_input(
            sheet,
            CellAddress::new(sheet, RowIndex(1), ColumnIndex(0)),
            CellInput::Number(2.0),
        )
        .unwrap();
        api.set_cell_input(
            sheet,
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(1)),
            CellInput::Text("b".into()),
        )
        .unwrap();
        api.set_cell_format(
            sheet,
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(0)),
            CellFormat::default(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
        let views = api.cell_views(sheet).unwrap();
        assert_eq!(views.len(), 3);
        assert_eq!(
            views[0].address,
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(0))
        );
        assert_eq!(views[0].input, CellInput::Empty);
        assert!(views[0].format.is_some());
        assert_eq!(
            views[1].address,
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(1))
        );
        assert_eq!(views[1].input, CellInput::Text("b".into()));
        assert_eq!(
            views[2].address,
            CellAddress::new(sheet, RowIndex(1), ColumnIndex(0))
        );
        assert_eq!(views[2].input, CellInput::Number(2.0));
    }

    #[test]
    fn cell_views_rejects_unknown_sheet() {
        let api = SpreadsheetApi::new(FakeEngine);
        let unknown = SheetId(999);
        let error = api.cell_views(unknown).unwrap_err();
        assert!(matches!(
            error.kind(),
            ApiErrorKind::Core(CoreError::SheetNotFound(_))
        ));
    }

    #[test]
    fn default_config_accepts_excel_max_row() {
        let config = DocumentConfig::default();
        assert!(config.normalize_row(1_048_575).is_ok());
        assert!(config.normalize_row(1_048_576).is_err());
    }

    #[test]
    fn replace_literal_avoids_no_change_transactions() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        api.set_cell_input(sheet, address, CellInput::Text("hello world".into()))
            .unwrap();
        let response = api.replace_literal("xyz", "abc", None, true).unwrap();
        assert_eq!(response.base_revision(), api.document.revision());
        assert!(response.calculated_cells().is_empty());
    }

    #[test]
    fn replace_literal_case_insensitive_replaces_all_matches() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        api.set_cell_input(sheet, address, CellInput::Text("İ i̇ I ı".into()))
            .unwrap();
        api.replace_literal("i", "X", None, false).unwrap();
        let result = api.cell_view(sheet, address).unwrap();
        assert_eq!(result.input, CellInput::Text("İ X\u{307} X ı".into()));
    }

    #[test]
    fn replace_literal_replaces_all_non_overlapping_ascii() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        api.set_cell_input(sheet, address, CellInput::Text("aaa".into()))
            .unwrap();
        api.replace_literal("a", "b", None, true).unwrap();
        let result = api.cell_view(sheet, address).unwrap();
        assert_eq!(result.input, CellInput::Text("bbb".into()));
    }

    #[test]
    fn replace_literal_unicode_non_ascii_boundaries() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        api.set_cell_input(sheet, address, CellInput::Text("café CAFÉ".into()))
            .unwrap();
        api.replace_literal("café", "tea", None, false).unwrap();
        let result = api.cell_view(sheet, address).unwrap();
        assert_eq!(result.input, CellInput::Text("tea CAFÉ".into()));
    }

    #[test]
    fn apply_format_range_preserves_default_base_and_borders() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        let base = CellFormat {
            font: sheets_core::Font {
                family: "Base".into(),
                size: 12,
                bold: true,
                italic: true,
                underline: true,
            },
            foreground: Some(sheets_core::Color::new(1, 2, 3, 4)),
            background: Some(sheets_core::Color::new(5, 6, 7, 8)),
            alignment: sheets_core::Alignment {
                horizontal: Some(sheets_core::HorizontalAlign::Center),
                vertical: Some(sheets_core::VerticalAlign::Middle),
                wrap: true,
            },
            number_format: sheets_core::NumberFormat::General,
            borders: sheets_core::Borders {
                top: Some(sheets_core::Border {
                    style: sheets_core::BorderStyle::Dashed,
                    color: Some(sheets_core::Color::new(9, 10, 11, 12)),
                }),
                bottom: Some(sheets_core::Border {
                    style: sheets_core::BorderStyle::Solid,
                    color: Some(sheets_core::Color::new(13, 14, 15, 16)),
                }),
                left: None,
                right: None,
            },
        };
        api.set_cell_format(
            sheet,
            address,
            base.clone(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();

        let patch = FormatPatch {
            bold: Some(false),
            ..FormatPatch::default()
        };
        api.apply_format_range(
            sheet,
            CellRange::new(address, address).unwrap(),
            patch,
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();

        let format = api.document.cell_format(sheet, address).unwrap();
        assert_eq!(format.font.family, "Base");
        assert_eq!(format.font.size, 12);
        assert!(!format.font.bold);
        assert!(format.font.italic);
        assert!(format.font.underline);
        assert_eq!(format.foreground, Some(sheets_core::Color::new(1, 2, 3, 4)));
        assert_eq!(format.background, Some(sheets_core::Color::new(5, 6, 7, 8)));
        assert_eq!(format.number_format, sheets_core::NumberFormat::General);
        assert_eq!(
            format.borders.top.as_ref().unwrap().style,
            sheets_core::BorderStyle::Dashed
        );
        assert_eq!(
            format.borders.bottom.as_ref().unwrap().style,
            sheets_core::BorderStyle::Solid
        );
    }

    #[test]
    fn apply_format_range_uses_default_base_for_missing_format() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));

        api.apply_format_range(
            sheet,
            CellRange::new(address, address).unwrap(),
            FormatPatch {
                font_family: Some("New".into()),
                ..FormatPatch::default()
            },
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();

        let format = api.document.cell_format(sheet, address).unwrap();
        assert_eq!(format.font.family, "New");
        assert_eq!(format.font.size, 11);
        assert_eq!(format.borders.top, None);
    }

    #[test]
    fn apply_format_range_validates_inputs_before_mutation() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        let range = CellRange::new(address, address).unwrap();
        let start_revision = api.document.revision();

        let bad_patch = FormatPatch {
            font_family: Some("  ".into()),
            ..FormatPatch::default()
        };
        let result = api.apply_format_range(
            sheet,
            range.clone(),
            bad_patch,
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        );
        assert!(result.is_err());
        assert_eq!(api.document.revision(), start_revision);
        assert!(api.document.cell_format(sheet, address).is_none());

        let mut bad_range = range;
        bad_range.end = CellAddress::new(sheet, RowIndex(10), ColumnIndex(0));
        let result = api.apply_format_range(
            sheet,
            bad_range.clone(),
            FormatPatch::default(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        );
        assert!(result.is_err());
        assert_eq!(api.document.revision(), start_revision);
    }

    #[test]
    fn apply_format_range_rejects_zero_and_oversized_bounds() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let base_range = CellRange::new(
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(0)),
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(0)),
        )
        .unwrap();
        let oversized_range = CellRange::new(
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(0)),
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(1)),
        )
        .unwrap();

        assert!(matches!(
            api.apply_format_range(
                sheet,
                base_range.clone(),
                FormatPatch::default(),
                FormatPatchBounds {
                    max_cells: 0,
                    max_sheets: 1
                }
            )
            .unwrap_err()
            .kind(),
            ApiErrorKind::Unsupported(_)
        ));
        assert!(matches!(
            api.apply_format_range(
                sheet,
                base_range.clone(),
                FormatPatch::default(),
                FormatPatchBounds {
                    max_cells: 1,
                    max_sheets: 0
                }
            )
            .unwrap_err()
            .kind(),
            ApiErrorKind::Unsupported(_)
        ));
        assert!(matches!(
            api.apply_format_range(
                sheet,
                oversized_range,
                FormatPatch::default(),
                FormatPatchBounds {
                    max_cells: 1,
                    max_sheets: 1
                }
            )
            .unwrap_err()
            .kind(),
            ApiErrorKind::Transaction(ApiTransactionError::DocumentLimitExceeded(_))
        ));
    }

    #[test]
    fn apply_format_range_rejects_invalid_target_sheet() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let other = {
            let mut wb = Workbook::new();
            let mut changes = ChangeSet::new();
            changes.push(sheets_core::Change::AddSheet {
                name: Some("Other".into()),
            });
            let mut plan = wb.prepare(changes).unwrap();
            wb.commit(&mut plan, EngineResponse::new(Revision(0), Vec::new()))
                .unwrap();
            wb.active_sheet()
        };
        let range = CellRange::new(
            CellAddress::new(other, RowIndex(0), ColumnIndex(0)),
            CellAddress::new(other, RowIndex(0), ColumnIndex(0)),
        )
        .unwrap();
        let result = api.apply_format_range(
            sheet,
            range,
            FormatPatch {
                font_family: Some("x".into()),
                ..FormatPatch::default()
            },
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        );
        assert!(matches!(
            result.unwrap_err().kind(),
            ApiErrorKind::Core(CoreError::InvalidRange(_))
        ));
    }

    #[test]
    fn apply_format_range_valid_patch_empty_is_noop_after_validation() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let range = CellRange::new(
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(0)),
            CellAddress::new(sheet, RowIndex(0), ColumnIndex(0)),
        )
        .unwrap();
        let start = api.document.revision();
        let response = api
            .apply_format_range(
                sheet,
                range,
                FormatPatch::default(),
                FormatPatchBounds {
                    max_cells: 1,
                    max_sheets: 1,
                },
            )
            .unwrap();
        assert_eq!(response.base_revision(), start);
        assert!(response.calculated_cells().is_empty());
    }

    #[test]
    fn apply_format_range_patch_merges_row_major_once() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let a1 = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        let b1 = CellAddress::new(sheet, RowIndex(0), ColumnIndex(1));
        let a2 = CellAddress::new(sheet, RowIndex(1), ColumnIndex(0));
        let b2 = CellAddress::new(sheet, RowIndex(1), ColumnIndex(1));
        let range = CellRange::new(a1, b2).unwrap();

        api.set_cell_input(sheet, a1, CellInput::Text("a1".into()))
            .unwrap();
        api.set_cell_input(sheet, b1, CellInput::Text("b1".into()))
            .unwrap();
        api.set_cell_input(sheet, a2, CellInput::Text("a2".into()))
            .unwrap();
        api.set_cell_input(sheet, b2, CellInput::Text("b2".into()))
            .unwrap();

        let response = api
            .apply_format_range(
                sheet,
                range.clone(),
                FormatPatch {
                    font_family: Some("Mono".into()),
                    bold: Some(true),
                    ..FormatPatch::default()
                },
                FormatPatchBounds {
                    max_cells: 10,
                    max_sheets: 1,
                },
            )
            .unwrap();
        let new_revision = response.base_revision();

        assert_eq!(
            api.document.cell_format(sheet, a1).unwrap().font.family,
            "Mono"
        );
        assert_eq!(
            api.document.cell_format(sheet, b1).unwrap().font.family,
            "Mono"
        );
        assert_eq!(
            api.document.cell_format(sheet, a2).unwrap().font.family,
            "Mono"
        );
        assert_eq!(
            api.document.cell_format(sheet, b2).unwrap().font.family,
            "Mono"
        );
        assert!(api.document.cell_format(sheet, a1).unwrap().font.bold);

        let another = api
            .apply_format_range(
                sheet,
                range.clone(),
                FormatPatch {
                    italic: Some(true),
                    ..FormatPatch::default()
                },
                FormatPatchBounds {
                    max_cells: 10,
                    max_sheets: 1,
                },
            )
            .unwrap();
        assert_eq!(
            another.base_revision(),
            new_revision.checked_next().unwrap()
        );

        assert_eq!(
            api.document.cell_format(sheet, a1).unwrap().font.family,
            "Mono"
        );
        assert!(api.document.cell_format(sheet, b1).unwrap().font.bold);
        assert!(api.document.cell_format(sheet, a2).unwrap().font.italic);
        assert!(api.document.cell_format(sheet, b2).unwrap().font.italic);
    }

    #[test]
    fn number_format_parser_is_case_insensitive_and_preserves_custom_payload() {
        assert_eq!(
            parse_number_format("CUSTOM:0x00FF").unwrap(),
            sheets_core::NumberFormat::Custom("0x00FF".into())
        );
        assert_eq!(
            parse_number_format("CuStOm:abc").unwrap(),
            sheets_core::NumberFormat::Custom("abc".into())
        );
        assert_eq!(
            parse_number_format("CUSTOM:AbC;[Red]0").unwrap(),
            sheets_core::NumberFormat::Custom("AbC;[Red]0".into())
        );
        assert_eq!(
            parse_number_format("DATE").unwrap(),
            sheets_core::NumberFormat::Date
        );
        assert_eq!(
            parse_number_format("TIME").unwrap(),
            sheets_core::NumberFormat::Time
        );
    }

    #[test]
    fn cell_view_display_and_raw_text() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        api.set_cell_input(sheet, address, CellInput::Text("hello".into()))
            .unwrap();
        let view = api.cell_view(sheet, address).unwrap();
        assert_eq!(view.display_text(), "hello");
        assert_eq!(view.raw_text(), "hello");
    }

    #[test]
    fn cell_view_display_formula() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        api.set_cell_input(
            sheet,
            address,
            CellInput::formula("SUM(A1)".to_string()).unwrap(),
        )
        .unwrap();
        let view = api.cell_view(sheet, address).unwrap();
        assert_eq!(view.display_text(), "=SUM(A1)");
        assert_eq!(view.raw_text(), "=SUM(A1)");
    }

    #[test]
    fn load_xlsx_replaces_live_document_and_recalculates_formulas() {
        use sheets_ironcalc_adapter::IroncalcAdapter;

        let mut source = SpreadsheetApi::new(IroncalcAdapter::new());
        let source_sheet = source.document().active_sheet();
        let a1 = CellAddress::new(source_sheet, RowIndex(0), ColumnIndex(0));
        let b1 = CellAddress::new(source_sheet, RowIndex(0), ColumnIndex(1));
        let c1 = CellAddress::new(source_sheet, RowIndex(0), ColumnIndex(2));
        source
            .set_cell_input(source_sheet, a1, CellInput::Number(40.0))
            .unwrap();
        source
            .set_cell_input(source_sheet, b1, CellInput::Number(2.0))
            .unwrap();
        source
            .set_cell_input(
                source_sheet,
                c1,
                CellInput::formula("A1+B1".to_string()).unwrap(),
            )
            .unwrap();
        let bytes = source.export_xlsx().unwrap();

        let mut target = SpreadsheetApi::new(IroncalcAdapter::new());
        let target_sheet = target.document().active_sheet();
        target
            .set_cell_input(
                target_sheet,
                CellAddress::new(target_sheet, RowIndex(3), ColumnIndex(0)),
                CellInput::Text("old document".into()),
            )
            .unwrap();

        let loaded = target.load_xlsx(&bytes).unwrap();
        assert_eq!(loaded.sheets.len(), 1);
        assert_eq!(target.document().sheet_count(), 1);
        assert_eq!(
            target.document().cell_input(target_sheet, c1),
            Some(&CellInput::formula("A1+B1".to_string()).unwrap())
        );
        assert_eq!(
            target.document().cell_value(target_sheet, c1),
            Some(&sheets_core::CellValue::Number(42.0))
        );
        assert_eq!(
            target.document().cell_input(
                target_sheet,
                CellAddress::new(target_sheet, RowIndex(3), ColumnIndex(0))
            ),
            None
        );
    }

    #[test]
    fn apply_format_range_preserves_custom_number_format_payload() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        api.apply_format_range(
            sheet,
            CellRange::new(address, address).unwrap(),
            FormatPatch {
                number_format: Some("CUSTOM:AbC;[Red]0".into()),
                ..FormatPatch::default()
            },
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
        let format = api.document.cell_format(sheet, address).unwrap();
        assert_eq!(
            format.number_format,
            sheets_core::NumberFormat::Custom("AbC;[Red]0".into())
        );
    }

    #[test]
    fn apply_format_range_rejects_invalid_number_format() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        let error = api
            .apply_format_range(
                sheet,
                CellRange::new(address, address).unwrap(),
                FormatPatch {
                    number_format: Some("decimal:not-a-number".into()),
                    ..FormatPatch::default()
                },
                FormatPatchBounds {
                    max_cells: 1,
                    max_sheets: 1,
                },
            )
            .unwrap_err();
        assert!(matches!(error.kind(), ApiErrorKind::Format(_)));
    }

    #[test]
    fn apply_format_range_rejects_invalid_font_size() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        let error = api
            .apply_format_range(
                sheet,
                CellRange::new(address, address).unwrap(),
                FormatPatch {
                    font_size: Some(0.5),
                    ..FormatPatch::default()
                },
                FormatPatchBounds {
                    max_cells: 1,
                    max_sheets: 1,
                },
            )
            .unwrap_err();
        assert!(matches!(error.kind(), ApiErrorKind::Format(_)));
    }

    #[test]
    fn apply_format_range_rejects_unknown_sheet_pre_transaction() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let unknown = SheetId(999);
        let address = CellAddress::new(unknown, RowIndex(0), ColumnIndex(0));
        let range = CellRange::new(address, address).unwrap();
        let error = api
            .apply_format_range(
                unknown,
                range,
                FormatPatch {
                    bold: Some(true),
                    ..FormatPatch::default()
                },
                FormatPatchBounds {
                    max_cells: 1,
                    max_sheets: 1,
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ApiErrorKind::Core(CoreError::SheetNotFound(_))
        ));
    }

    #[test]
    fn apply_format_range_bold_preserves_borders_and_increments_revision() {
        let mut api = SpreadsheetApi::new(FakeEngine);
        let sheet = api.document.active_sheet();
        let a1 = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        let b1 = CellAddress::new(sheet, RowIndex(0), ColumnIndex(1));
        let a2 = CellAddress::new(sheet, RowIndex(1), ColumnIndex(0));
        let b2 = CellAddress::new(sheet, RowIndex(1), ColumnIndex(1));
        let base = CellFormat {
            borders: sheets_core::Borders {
                top: Some(sheets_core::Border {
                    style: sheets_core::BorderStyle::Dashed,
                    color: Some(sheets_core::Color::new(1, 2, 3, 4)),
                }),
                bottom: None,
                left: None,
                right: None,
            },
            ..CellFormat::default()
        };
        api.set_cell_format(
            sheet,
            a1,
            base.clone(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
        api.set_cell_format(
            sheet,
            b1,
            base.clone(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
        api.set_cell_format(
            sheet,
            a2,
            base.clone(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
        api.set_cell_format(
            sheet,
            b2,
            base.clone(),
            FormatPatchBounds {
                max_cells: 1,
                max_sheets: 1,
            },
        )
        .unwrap();
        let start = api.document.revision();
        let response = api
            .apply_format_range(
                sheet,
                CellRange::new(a1, b2).unwrap(),
                FormatPatch {
                    bold: Some(true),
                    ..FormatPatch::default()
                },
                FormatPatchBounds {
                    max_cells: 10,
                    max_sheets: 1,
                },
            )
            .unwrap();
        assert_eq!(response.base_revision(), start);
        assert_eq!(api.document.revision(), start.checked_next().unwrap());
        assert_eq!(
            api.document
                .cell_format(sheet, a1)
                .unwrap()
                .borders
                .top
                .as_ref()
                .unwrap()
                .style,
            sheets_core::BorderStyle::Dashed
        );
        assert!(api.document.cell_format(sheet, b2).unwrap().font.bold);
        assert_eq!(
            api.document
                .cell_format(sheet, a2)
                .unwrap()
                .borders
                .top
                .as_ref()
                .unwrap()
                .style,
            sheets_core::BorderStyle::Dashed
        );
    }
}
