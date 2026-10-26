//! Adapter between the canonical Sheets model and the local IronCalc engine.
//!
//! Core owns mutations and snapshots; this crate rebuilds an IronCalc model for
//! each evaluation, so every formula is parsed and calculated by IronCalc.

use std::error::Error;
use std::fmt;
use std::sync::OnceLock;

mod sorted_formulas;
pub use sorted_formulas::translate_sorted_formulas;
mod clipboard_formulas;
pub use clipboard_formulas::{
    build_native_cut_model, copied_formula_inputs, extract_native_cut_formulas,
    moved_formula_inputs,
};
mod structural;
pub use structural::{StructuralOperation, structural_inputs};

use ironcalc::base::{
    Model,
    cell::CellValue as IroncalcCellValue,
    expressions::{token::Error as IroncalcError, utils::number_to_column},
    formatter::format::format_number,
    locale::get_locale,
    types::{
        Alignment as IroncalcAlignment, Border as IroncalcBorder, BorderItem as IroncalcBorderItem,
        BorderStyle as IroncalcBorderStyle, Cell as IroncalcCell, Color as IroncalcColor,
        Fill as IroncalcFill, Font as IroncalcFont, FormulaValue,
        HorizontalAlignment as IroncalcHorizontalAlignment, Style as IroncalcStyle,
        Theme as IroncalcTheme, VerticalAlignment as IroncalcVerticalAlignment,
    },
};
use sheets_core::{
    Alignment, Border, BorderStyle, Borders, CalculatedCell, CellAddress, CellError, CellFormat,
    CellInput, CellRange, CellSnapshot, CellValue, Color, ColumnDimension, ColumnIndex,
    DefaultGridMetrics, EngineRequest, EngineResponse, Font, HorizontalAlign, NumberFormat,
    Revision, RowDimension, RowIndex, SheetId, SheetSnapshot, SpreadsheetEngine, VerticalAlign,
    WorkbookSnapshot,
};

const ENGINE_WORKBOOK_NAME: &str = "RudraOffice.xlsx";
const ENGINE_LOCALE: &str = "en";
const ENGINE_TIMEZONE: &str = "UTC";
const ENGINE_LANGUAGE: &str = "en";

/// Default axis sizes returned by the production engine, in unzoomed pixels.
///
/// IronCalc's constants are private. Querying an empty model keeps API and UI
/// defaults tied to the same public getters used by XLSX import. The result is
/// cached so creating a UI configuration does not rebuild an engine model.
pub fn default_grid_metrics() -> DefaultGridMetrics {
    static METRICS: OnceLock<DefaultGridMetrics> = OnceLock::new();
    *METRICS.get_or_init(|| {
        let model = Model::new_empty(
            ENGINE_WORKBOOK_NAME,
            ENGINE_LOCALE,
            ENGINE_TIMEZONE,
            ENGINE_LANGUAGE,
        )
        .expect("production engine defaults must initialize");
        DefaultGridMetrics {
            row_height: model
                .get_row_height(0, 1)
                .expect("first default row must have a height"),
            column_width: model
                .get_column_width(0, 1)
                .expect("first default column must have a width"),
        }
    })
}

/// Real IronCalc-backed formula engine.
#[derive(Clone, Copy, Debug, Default)]
pub struct IroncalcAdapter;

impl IroncalcAdapter {
    pub const fn new() -> Self {
        Self
    }

    pub const fn backend_name(&self) -> &'static str {
        "ironcalc-0.8.3"
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterError {
    Engine(String),
    Snapshot(String),
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(message) => write!(f, "IronCalc engine error: {message}"),
            Self::Snapshot(message) => write!(f, "IronCalc snapshot conversion error: {message}"),
        }
    }
}

impl Error for AdapterError {}

impl SpreadsheetEngine for IroncalcAdapter {
    type Error = AdapterError;

    fn evaluate(&mut self, request: EngineRequest) -> Result<EngineResponse, Self::Error> {
        let snapshot = request.workbook();
        let mut model = model_from_snapshot(snapshot)?;
        model.evaluate();

        let mut calculated_cells = Vec::new();
        for (sheet_index, sheet) in snapshot.sheets.iter().enumerate() {
            let native_sheet = native_sheet_index(sheet_index)?;
            for cell in &sheet.cells {
                if matches!(cell.input, CellInput::Formula(_)) {
                    calculated_cells.push(CalculatedCell {
                        address: cell.address,
                        value: calculated_value(&model, native_sheet, cell.address)?,
                    });
                }
            }
        }
        Ok(EngineResponse::new(
            request.base_revision(),
            calculated_cells,
        ))
    }
}

/// Builds an IronCalc model from the canonical Sheets snapshot.
///
/// It preserves sheet order and names, inputs, direct cell formatting,
/// dimensions, merges, and the active sheet. Row and column inherited styles,
/// conditional formatting, and named styles remain outside the canonical model.
pub fn model_from_snapshot(snapshot: &WorkbookSnapshot) -> Result<Model<'static>, AdapterError> {
    let first_sheet = snapshot
        .sheets
        .first()
        .ok_or_else(|| AdapterError::Snapshot("workbook must contain at least one sheet".into()))?;
    let mut model = Model::new_empty(
        ENGINE_WORKBOOK_NAME,
        ENGINE_LOCALE,
        ENGINE_TIMEZONE,
        ENGINE_LANGUAGE,
    )
    .map_err(AdapterError::Engine)?;

    model
        .rename_sheet_by_index(0, &first_sheet.name)
        .map_err(AdapterError::Engine)?;
    for sheet in snapshot.sheets.iter().skip(1) {
        model.add_sheet(&sheet.name).map_err(AdapterError::Engine)?;
    }

    for (sheet_index, sheet) in snapshot.sheets.iter().enumerate() {
        let native_sheet = native_sheet_index(sheet_index)?;
        for cell in &sheet.cells {
            if cell.address.sheet != sheet.id {
                return Err(AdapterError::Snapshot(format!(
                    "cell address belongs to sheet {}, not {}",
                    cell.address.sheet, sheet.id
                )));
            }
            let row = native_coordinate(cell.address.row.0, "row")?;
            let column = native_coordinate(cell.address.column.0, "column")?;
            model
                .set_user_input(native_sheet, row, column, ironcalc_input(&cell.input))
                .map_err(AdapterError::Engine)?;
            if let Some(format) = &cell.format {
                model
                    .set_cell_style(
                        native_sheet,
                        row,
                        column,
                        &ironcalc_style_from_cell_format(format),
                    )
                    .map_err(AdapterError::Engine)?;
            }
        }
        for dimension in &sheet.row_dimensions {
            model
                .set_row_height(
                    native_sheet,
                    native_coordinate(dimension.row.0, "row")?,
                    dimension.height,
                )
                .map_err(AdapterError::Engine)?;
        }
        for dimension in &sheet.column_dimensions {
            model
                .set_column_width(
                    native_sheet,
                    native_coordinate(dimension.column.0, "column")?,
                    dimension.width,
                )
                .map_err(AdapterError::Engine)?;
        }
        let native_worksheet = model
            .workbook
            .worksheet_mut(native_sheet)
            .map_err(AdapterError::Engine)?;
        for merge in &sheet.merges {
            if merge.sheet != sheet.id {
                return Err(AdapterError::Snapshot(format!(
                    "merge range belongs to sheet {}, not {}",
                    merge.sheet, sheet.id
                )));
            }
            native_worksheet.merge_cells.push(cell_range_to_a1(merge)?);
        }
    }

    let active_index = snapshot
        .sheets
        .iter()
        .position(|sheet| sheet.id == snapshot.active_sheet)
        .ok_or_else(|| AdapterError::Snapshot("active sheet is not present".into()))?;
    let view = model
        .workbook
        .views
        .get_mut(&0)
        .ok_or_else(|| AdapterError::Engine("default workbook view is missing".into()))?;
    view.sheet = native_sheet_index(active_index)?;
    Ok(model)
}

/// Converts a parsed IronCalc workbook into the canonical Sheets snapshot.
pub fn snapshot_from_model(model: &Model<'_>) -> Result<WorkbookSnapshot, AdapterError> {
    if model.workbook.worksheets.is_empty() {
        return Err(AdapterError::Snapshot(
            "IronCalc workbook contains no worksheets".into(),
        ));
    }

    let defaults = default_grid_metrics();
    let mut sheets = Vec::with_capacity(model.workbook.worksheets.len());
    for (sheet_index, worksheet) in model.workbook.worksheets.iter().enumerate() {
        let sheet_id = snapshot_sheet_id(sheet_index)?;
        let native_sheet = native_sheet_index(sheet_index)?;
        let mut locations = Vec::new();
        for (row, columns) in &worksheet.sheet_data {
            for column in columns.keys() {
                locations.push((*row, *column));
            }
        }
        locations.sort_unstable();

        let mut cells = Vec::new();
        for (row, column) in locations {
            let cell = worksheet
                .cell(row, column)
                .ok_or_else(|| AdapterError::Snapshot("worksheet cell disappeared".into()))?;
            let input = input_from_ironcalc_cell(model, native_sheet, row, column, cell)?;
            let format = format_from_ironcalc_cell(model, native_sheet, row, column, cell)?;
            if input.is_some() || format.is_some() {
                cells.push(CellSnapshot {
                    address: snapshot_address(sheet_id, row, column)?,
                    input: input.unwrap_or(CellInput::Empty),
                    format,
                });
            }
        }

        let mut row_dimensions = Vec::new();
        for row in &worksheet.rows {
            // The visible getter returns zero for a hidden row. Keep its
            // underlying pixel height independent of manual visibility.
            let height = row.height * ironcalc::base::ROW_HEIGHT_FACTOR;
            // XLSX also stores calculated heights with customHeight=false.
            // Preserve their effective geometry, not just the editor flag.
            if row.custom_height || (height > 0.0 && height != defaults.row_height) {
                row_dimensions.push(RowDimension {
                    row: snapshot_row_index(row.r)?,
                    height,
                });
            }
        }
        row_dimensions.sort_by_key(|dimension| dimension.row);

        let mut column_dimensions = Vec::new();
        for column in &worksheet.cols {
            if !column.custom_width {
                continue;
            }
            for native_column in column.min..=column.max {
                column_dimensions.push(ColumnDimension {
                    column: snapshot_column_index(native_column)?,
                    // The visible getter returns zero for a hidden column.
                    // Canonical width and manual visibility are independent.
                    width: worksheet
                        .get_actual_column_width(native_column)
                        .map_err(AdapterError::Engine)?,
                });
            }
        }
        column_dimensions.sort_by_key(|dimension| dimension.column);
        column_dimensions.dedup_by_key(|dimension| dimension.column);

        let mut manual_hidden_columns = Vec::new();
        for column in &worksheet.cols {
            if !column.hidden {
                continue;
            }
            if column.min < 1 || column.max > 16_384 || column.min > column.max {
                return Err(AdapterError::Snapshot(
                    "invalid IronCalc hidden column range".into(),
                ));
            }
            for native_column in column.min..=column.max {
                manual_hidden_columns.push(snapshot_column_index(native_column)?);
            }
        }
        manual_hidden_columns.sort_unstable();
        manual_hidden_columns.dedup();

        let merges = worksheet
            .merge_cells
            .iter()
            .map(|merge| parse_a1_range(sheet_id, merge))
            .collect::<Result<Vec<_>, _>>()?;
        sheets.push(SheetSnapshot {
            id: sheet_id,
            name: worksheet.name.clone(),
            cells,
            merges,
            row_dimensions,
            column_dimensions,
            auto_filter: None,
            manual_hidden_rows: worksheet
                .rows
                .iter()
                .filter(|row| row.hidden)
                .map(|row| snapshot_row_index(row.r))
                .collect::<Result<Vec<_>, _>>()?,
            manual_hidden_columns,
        });
    }

    let active_index = model
        .workbook
        .views
        .get(&0)
        .map(|view| view.sheet as usize)
        .filter(|index| *index < sheets.len())
        .unwrap_or(0);
    Ok(WorkbookSnapshot {
        revision: Revision(0),
        active_sheet: snapshot_sheet_id(active_index)?,
        sheets,
    })
}

fn ironcalc_style_from_cell_format(format: &CellFormat) -> IroncalcStyle {
    IroncalcStyle {
        alignment: ironcalc_alignment(&format.alignment),
        num_fmt: ironcalc_number_format(&format.number_format),
        fill: IroncalcFill {
            color: ironcalc_color(format.background.as_ref()),
        },
        font: IroncalcFont {
            b: format.font.bold,
            i: format.font.italic,
            u: format.font.underline,
            sz: i32::from(format.font.size),
            color: ironcalc_color(format.foreground.as_ref()),
            name: format.font.family.clone(),
            ..IroncalcFont::default()
        },
        border: IroncalcBorder {
            top: ironcalc_border_item(format.borders.top.as_ref()),
            right: ironcalc_border_item(format.borders.right.as_ref()),
            bottom: ironcalc_border_item(format.borders.bottom.as_ref()),
            left: ironcalc_border_item(format.borders.left.as_ref()),
            ..IroncalcBorder::default()
        },
        quote_prefix: false,
    }
}

fn cell_format_from_ironcalc(style: &IroncalcStyle, theme: &IroncalcTheme) -> CellFormat {
    CellFormat {
        font: Font {
            family: style.font.name.clone(),
            size: u16::try_from(style.font.sz)
                .ok()
                .filter(|size| *size > 0)
                .unwrap_or(11),
            bold: style.font.b,
            italic: style.font.i,
            underline: style.font.u,
        },
        foreground: core_color(&style.font.color, theme),
        background: core_color(&style.fill.color, theme),
        borders: Borders {
            top: core_border_item(style.border.top.as_ref(), theme),
            right: core_border_item(style.border.right.as_ref(), theme),
            bottom: core_border_item(style.border.bottom.as_ref(), theme),
            left: core_border_item(style.border.left.as_ref(), theme),
        },
        alignment: core_alignment(style.alignment.as_ref()),
        number_format: core_number_format(&style.num_fmt),
    }
}

fn format_from_ironcalc_cell(
    model: &Model<'_>,
    sheet: u32,
    row: i32,
    column: i32,
    cell: &IroncalcCell,
) -> Result<Option<CellFormat>, AdapterError> {
    if cell.get_style() == 0 {
        return Ok(None);
    }
    model
        .get_style_for_cell(sheet, row, column)
        .map(|style| Some(cell_format_from_ironcalc(&style, &model.workbook.theme)))
        .map_err(AdapterError::Engine)
}

fn ironcalc_color(color: Option<&Color>) -> IroncalcColor {
    match color {
        Some(color) => {
            IroncalcColor::Rgb(format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b))
        }
        None => IroncalcColor::None,
    }
}

fn core_color(color: &IroncalcColor, theme: &IroncalcTheme) -> Option<Color> {
    let value = color.to_rgb(theme);
    let value = value.strip_prefix('#')?;
    if value.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&value[0..2], 16).ok()?;
    let g = u8::from_str_radix(&value[2..4], 16).ok()?;
    let b = u8::from_str_radix(&value[4..6], 16).ok()?;
    Some(Color::new(r, g, b, 255))
}

fn ironcalc_border_item(border: Option<&Border>) -> Option<IroncalcBorderItem> {
    let border = border?;
    let style = match border.style {
        BorderStyle::None => return None,
        BorderStyle::Solid => IroncalcBorderStyle::Thin,
        BorderStyle::Dashed => IroncalcBorderStyle::MediumDashed,
    };
    Some(IroncalcBorderItem {
        style,
        color: ironcalc_color(border.color.as_ref()),
    })
}

fn core_border_item(border: Option<&IroncalcBorderItem>, theme: &IroncalcTheme) -> Option<Border> {
    border.map(|border| Border {
        style: match border.style {
            IroncalcBorderStyle::Thin
            | IroncalcBorderStyle::Medium
            | IroncalcBorderStyle::Thick
            | IroncalcBorderStyle::Double => BorderStyle::Solid,
            IroncalcBorderStyle::Dotted
            | IroncalcBorderStyle::SlantDashDot
            | IroncalcBorderStyle::MediumDashed
            | IroncalcBorderStyle::MediumDashDotDot
            | IroncalcBorderStyle::MediumDashDot => BorderStyle::Dashed,
        },
        color: core_color(&border.color, theme),
    })
}

fn ironcalc_alignment(alignment: &Alignment) -> Option<IroncalcAlignment> {
    if alignment == &Alignment::default() {
        return None;
    }
    Some(IroncalcAlignment {
        horizontal: match alignment.horizontal {
            Some(HorizontalAlign::Left) => IroncalcHorizontalAlignment::Left,
            Some(HorizontalAlign::Center) => IroncalcHorizontalAlignment::Center,
            Some(HorizontalAlign::Right) => IroncalcHorizontalAlignment::Right,
            None => IroncalcHorizontalAlignment::General,
        },
        vertical: match alignment.vertical {
            Some(VerticalAlign::Top) => IroncalcVerticalAlignment::Top,
            Some(VerticalAlign::Middle) => IroncalcVerticalAlignment::Center,
            Some(VerticalAlign::Bottom) => IroncalcVerticalAlignment::Bottom,
            None => IroncalcVerticalAlignment::Bottom,
        },
        wrap_text: alignment.wrap,
    })
}

fn core_alignment(alignment: Option<&IroncalcAlignment>) -> Alignment {
    let Some(alignment) = alignment else {
        return Alignment::default();
    };
    Alignment {
        horizontal: match alignment.horizontal {
            IroncalcHorizontalAlignment::Left => Some(HorizontalAlign::Left),
            IroncalcHorizontalAlignment::Center | IroncalcHorizontalAlignment::CenterContinuous => {
                Some(HorizontalAlign::Center)
            }
            IroncalcHorizontalAlignment::Right => Some(HorizontalAlign::Right),
            IroncalcHorizontalAlignment::Distributed
            | IroncalcHorizontalAlignment::Fill
            | IroncalcHorizontalAlignment::General
            | IroncalcHorizontalAlignment::Justify => None,
        },
        vertical: match alignment.vertical {
            IroncalcVerticalAlignment::Top => Some(VerticalAlign::Top),
            IroncalcVerticalAlignment::Center => Some(VerticalAlign::Middle),
            IroncalcVerticalAlignment::Bottom => Some(VerticalAlign::Bottom),
            IroncalcVerticalAlignment::Distributed | IroncalcVerticalAlignment::Justify => None,
        },
        wrap: alignment.wrap_text,
    }
}

fn ironcalc_number_format(number_format: &NumberFormat) -> String {
    match number_format {
        NumberFormat::General => "general".to_string(),
        NumberFormat::Decimal(precision) => decimal_pattern(*precision),
        NumberFormat::Percent(precision) => format!("{}%", decimal_pattern(*precision)),
        NumberFormat::Currency { symbol, precision } => {
            format!(
                "\"{}\"#,##{}",
                symbol.replace('"', "\"\""),
                decimal_pattern(*precision)
            )
        }
        NumberFormat::Date => "yyyy-mm-dd".to_string(),
        NumberFormat::Time => "hh:mm:ss".to_string(),
        NumberFormat::Custom(value) => value.clone(),
    }
}

/// Format a finite numeric cell value with IronCalc's own number formatter.
/// Returns `None` for General, nonnumeric values, and invalid custom formats
/// so the caller can retain its existing ordinary display behavior.
pub fn formatted_numeric_display(value: &CellValue, format: &NumberFormat) -> Option<String> {
    if matches!(format, NumberFormat::General) {
        return None;
    }
    let serial = match value {
        CellValue::Number(value)
        | CellValue::Percent(value)
        | CellValue::Date(value)
        | CellValue::Time(value) => *value,
        CellValue::Empty | CellValue::Text(_) | CellValue::Boolean(_) | CellValue::Error(_) => {
            return None;
        }
    };
    if !serial.is_finite() {
        return None;
    }
    let locale = get_locale(ENGINE_LOCALE).ok()?;
    let formatted = format_number(serial, &ironcalc_number_format(format), locale);
    formatted.error.is_none().then_some(formatted.text)
}

pub(crate) fn core_number_format(value: &str) -> NumberFormat {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("general") {
        return NumberFormat::General;
    }
    if trimmed.eq_ignore_ascii_case("yyyy-mm-dd") {
        return NumberFormat::Date;
    }
    if trimmed.eq_ignore_ascii_case("hh:mm:ss") {
        return NumberFormat::Time;
    }
    if let Some(precision) = trimmed.strip_suffix('%').and_then(decimal_precision) {
        return NumberFormat::Percent(precision);
    }
    if let Some(number_format) = currency_number_format(trimmed) {
        return number_format;
    }
    if let Some(precision) = decimal_precision(trimmed) {
        return NumberFormat::Decimal(precision);
    }
    NumberFormat::Custom(value.to_string())
}

fn decimal_pattern(precision: u8) -> String {
    if precision == 0 {
        "0".to_string()
    } else {
        format!("0.{}", "0".repeat(usize::from(precision)))
    }
}

fn decimal_precision(value: &str) -> Option<u8> {
    if value == "0" {
        return Some(0);
    }
    let fraction = value.strip_prefix("0.")?;
    if fraction.is_empty() || !fraction.bytes().all(|byte| byte == b'0') {
        return None;
    }
    u8::try_from(fraction.len()).ok()
}

fn currency_number_format(value: &str) -> Option<NumberFormat> {
    let body = value.strip_prefix('"')?;
    let closing = body.find('"')?;
    let symbol = body[..closing].replace("\"\"", "\"");
    if symbol.is_empty() {
        return None;
    }
    let number = body[closing + 1..].strip_prefix("#,##")?;
    let precision = decimal_precision(number)?;
    Some(NumberFormat::Currency { symbol, precision })
}
fn calculated_value(
    model: &Model<'_>,
    sheet: u32,
    address: CellAddress,
) -> Result<CellValue, AdapterError> {
    let row = native_coordinate(address.row.0, "row")?;
    let column = native_coordinate(address.column.0, "column")?;
    let cell = model
        .workbook
        .worksheet(sheet)
        .map_err(AdapterError::Engine)?
        .cell(row, column)
        .ok_or_else(|| AdapterError::Engine("formula cell is missing after evaluation".into()))?;
    match cell {
        IroncalcCell::CellFormula { v, .. } | IroncalcCell::ArrayFormula { v, .. } => {
            if let FormulaValue::Error { ei, .. } = v {
                return Ok(CellValue::Error(core_error(ei)));
            }
            if matches!(v, FormulaValue::Unevaluated) {
                return Err(AdapterError::Engine(
                    "IronCalc left a formula unevaluated".into(),
                ));
            }
            model
                .get_cell_value_by_index(sheet, row, column)
                .map(ironcalc_value)
                .map_err(AdapterError::Engine)
        }
        _ => Err(AdapterError::Engine(
            "expected an IronCalc formula cell after evaluation".into(),
        )),
    }
}

fn input_from_ironcalc_cell(
    model: &Model<'_>,
    sheet: u32,
    row: i32,
    column: i32,
    cell: &IroncalcCell,
) -> Result<Option<CellInput>, AdapterError> {
    match cell {
        IroncalcCell::EmptyCell { .. } | IroncalcCell::SpillCell { .. } => Ok(None),
        IroncalcCell::BooleanCell { v, .. } => Ok(Some(CellInput::Boolean(*v))),
        IroncalcCell::NumberCell { v, .. } => Ok(Some(CellInput::Number(*v))),
        IroncalcCell::ErrorCell { ei, .. } => Ok(Some(CellInput::Text(ei.to_string()))),
        IroncalcCell::SharedString { si, .. } => model
            .workbook
            .shared_strings
            .get(*si as usize)
            .cloned()
            .map(CellInput::Text)
            .map(Some)
            .ok_or_else(|| AdapterError::Snapshot("shared string index is invalid".into())),
        IroncalcCell::CellFormula { .. } | IroncalcCell::ArrayFormula { .. } => {
            let formula = model
                .get_cell_formula(sheet, row, column)
                .map_err(AdapterError::Engine)?
                .ok_or_else(|| AdapterError::Snapshot("formula cell has no formula text".into()))?;
            CellInput::formula(formula)
                .map(Some)
                .map_err(|error| AdapterError::Snapshot(error.to_string()))
        }
    }
}

fn ironcalc_input(input: &CellInput) -> String {
    match input {
        CellInput::Empty => String::new(),
        CellInput::Text(value) => format!("'{value}"),
        CellInput::Number(value) | CellInput::Date(value) | CellInput::Time(value) => {
            value.to_string()
        }
        CellInput::Percent(value) => format!("{}%", value * 100.0),
        CellInput::Boolean(value) => value.to_string(),
        CellInput::Formula(value) => format!("={value}"),
    }
}

fn ironcalc_value(value: IroncalcCellValue) -> CellValue {
    match value {
        IroncalcCellValue::None => CellValue::Empty,
        IroncalcCellValue::String(value) => CellValue::Text(value),
        IroncalcCellValue::Number(value) => CellValue::Number(value),
        IroncalcCellValue::Boolean(value) => CellValue::Boolean(value),
    }
}

fn core_error(error: &IroncalcError) -> CellError {
    match error {
        IroncalcError::DIV => CellError::Div0,
        IroncalcError::REF => CellError::Ref,
        IroncalcError::NAME => CellError::Name,
        IroncalcError::NA => CellError::NA,
        IroncalcError::CIRC => CellError::Circular,
        IroncalcError::VALUE
        | IroncalcError::NUM
        | IroncalcError::ERROR
        | IroncalcError::NIMPL
        | IroncalcError::SPILL
        | IroncalcError::CALC
        | IroncalcError::NULL => CellError::Value,
    }
}

fn native_sheet_index(index: usize) -> Result<u32, AdapterError> {
    u32::try_from(index)
        .map_err(|_| AdapterError::Snapshot("too many worksheets for IronCalc".into()))
}

fn snapshot_sheet_id(index: usize) -> Result<SheetId, AdapterError> {
    u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .map(SheetId)
        .ok_or_else(|| AdapterError::Snapshot("worksheet identifier overflow".into()))
}

fn native_coordinate(index: u32, kind: &str) -> Result<i32, AdapterError> {
    i32::try_from(u64::from(index) + 1).map_err(|_| {
        AdapterError::Snapshot(format!(
            "{kind} index {index} exceeds IronCalc coordinate bounds"
        ))
    })
}

fn snapshot_row_index(row: i32) -> Result<RowIndex, AdapterError> {
    row.checked_sub(1)
        .and_then(|value| u32::try_from(value).ok())
        .map(RowIndex)
        .ok_or_else(|| AdapterError::Snapshot("invalid IronCalc row index".into()))
}

fn snapshot_column_index(column: i32) -> Result<ColumnIndex, AdapterError> {
    column
        .checked_sub(1)
        .and_then(|value| u32::try_from(value).ok())
        .map(ColumnIndex)
        .ok_or_else(|| AdapterError::Snapshot("invalid IronCalc column index".into()))
}

fn snapshot_address(sheet: SheetId, row: i32, column: i32) -> Result<CellAddress, AdapterError> {
    Ok(CellAddress::new(
        sheet,
        snapshot_row_index(row)?,
        snapshot_column_index(column)?,
    ))
}

fn cell_range_to_a1(range: &CellRange) -> Result<String, AdapterError> {
    range
        .validate()
        .map_err(|error| AdapterError::Snapshot(error.to_string()))?;
    Ok(format!(
        "{}:{}",
        cell_address_to_a1(range.start)?,
        cell_address_to_a1(range.end)?
    ))
}

fn cell_address_to_a1(address: CellAddress) -> Result<String, AdapterError> {
    let row = native_coordinate(address.row.0, "row")?;
    let column = native_coordinate(address.column.0, "column")?;
    let column = number_to_column(column).ok_or_else(|| {
        AdapterError::Snapshot("column cannot be represented in A1 notation".into())
    })?;
    Ok(format!("{column}{row}"))
}

fn parse_a1_range(sheet: SheetId, value: &str) -> Result<CellRange, AdapterError> {
    let mut parts = value.split(':');
    let start = parts
        .next()
        .ok_or_else(|| AdapterError::Snapshot("empty merge range".into()))?;
    let end = parts.next().unwrap_or(start);
    if parts.next().is_some() {
        return Err(AdapterError::Snapshot(format!(
            "invalid merge range '{value}'"
        )));
    }
    CellRange::new(
        parse_a1_address(sheet, start)?,
        parse_a1_address(sheet, end)?,
    )
    .map_err(|error| AdapterError::Snapshot(error.to_string()))
}

fn parse_a1_address(sheet: SheetId, value: &str) -> Result<CellAddress, AdapterError> {
    let mut letters = String::new();
    let mut digits = String::new();
    let mut in_digits = false;
    for character in value.trim().chars() {
        if character == '$' {
            continue;
        }
        if character.is_ascii_alphabetic() && !in_digits {
            letters.push(character.to_ascii_uppercase());
        } else if character.is_ascii_digit() {
            in_digits = true;
            digits.push(character);
        } else {
            return Err(AdapterError::Snapshot(format!(
                "invalid A1 address '{value}'"
            )));
        }
    }
    if letters.is_empty() || digits.is_empty() {
        return Err(AdapterError::Snapshot(format!(
            "invalid A1 address '{value}'"
        )));
    }

    let mut column = 0_u64;
    for letter in letters.bytes() {
        column = column
            .checked_mul(26)
            .and_then(|current| current.checked_add(u64::from(letter - b'A' + 1)))
            .ok_or_else(|| AdapterError::Snapshot("A1 column overflow".into()))?;
    }
    let row = digits
        .parse::<u64>()
        .map_err(|_| AdapterError::Snapshot(format!("invalid A1 row in '{value}'")))?;
    let row = row
        .checked_sub(1)
        .ok_or_else(|| AdapterError::Snapshot("A1 rows start at one".into()))?;
    let column = column
        .checked_sub(1)
        .ok_or_else(|| AdapterError::Snapshot("A1 columns start at A".into()))?;
    let row = u32::try_from(row)
        .map_err(|_| AdapterError::Snapshot("A1 row exceeds Sheets bounds".into()))?;
    let column = u32::try_from(column)
        .map_err(|_| AdapterError::Snapshot("A1 column exceeds Sheets bounds".into()))?;
    Ok(CellAddress::new(sheet, RowIndex(row), ColumnIndex(column)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_grid_metrics_match_production_axis_defaults() {
        let metrics = default_grid_metrics();
        assert_eq!(metrics.row_height, 25.0);
        assert_eq!(metrics.column_width, 90.0);
    }

    #[test]
    fn snapshot_preserves_effective_row_height_without_custom_flag() {
        let mut model = Model::new_empty(
            ENGINE_WORKBOOK_NAME,
            ENGINE_LOCALE,
            ENGINE_TIMEZONE,
            ENGINE_LANGUAGE,
        )
        .unwrap();
        model.set_row_height(0, 2, 48.75).unwrap();
        model.set_column_width(0, 3, 126.0).unwrap();
        let worksheet = &mut model.workbook.worksheets[0];
        worksheet
            .rows
            .iter_mut()
            .find(|row| row.r == 2)
            .unwrap()
            .custom_height = false;
        let snapshot = snapshot_from_model(&model).unwrap();
        assert_eq!(
            snapshot.sheets[0].row_dimensions,
            vec![RowDimension {
                row: RowIndex(1),
                height: 48.75
            }]
        );
        assert_eq!(
            snapshot.sheets[0].column_dimensions,
            vec![ColumnDimension {
                column: ColumnIndex(2),
                width: 126.0
            }]
        );
        let reopened = model_from_snapshot(&snapshot).unwrap();
        assert_eq!(reopened.get_row_height(0, 2).unwrap(), 48.75);
        assert_eq!(reopened.get_column_width(0, 3).unwrap(), 126.0);
    }

    #[test]
    fn snapshot_imports_hidden_row_height_and_visibility() {
        let mut model = Model::new_empty(
            ENGINE_WORKBOOK_NAME,
            ENGINE_LOCALE,
            ENGINE_TIMEZONE,
            ENGINE_LANGUAGE,
        )
        .expect("model");
        model.set_row_height(0, 2, 48.75).expect("height");
        model.set_row_hidden(0, 2, true).expect("hide row");

        let snapshot = snapshot_from_model(&model).expect("snapshot");
        assert_eq!(snapshot.sheets[0].manual_hidden_rows, vec![RowIndex(1)]);
        assert_eq!(
            snapshot.sheets[0].row_dimensions,
            vec![RowDimension {
                row: RowIndex(1),
                height: 48.75,
            }]
        );
    }

    use sheets_core::{Change, ChangeSet, Workbook};

    fn address(sheet: SheetId, row: u32, column: u32) -> CellAddress {
        CellAddress::new(sheet, RowIndex(row), ColumnIndex(column))
    }

    fn formula(raw: &str) -> CellInput {
        CellInput::formula(raw).expect("valid formula")
    }

    fn request_with(cells: &[(CellAddress, CellInput)]) -> EngineRequest {
        let workbook = Workbook::new();
        let sheet = workbook.active_sheet();
        let changes = cells
            .iter()
            .fold(ChangeSet::new(), |mut changes, (address, input)| {
                changes.push(Change::SetCellInput {
                    sheet,
                    address: *address,
                    input: input.clone(),
                });
                changes
            });
        workbook
            .prepare(changes)
            .expect("valid changes")
            .engine_request()
    }

    fn value_for(response: &EngineResponse, address: CellAddress) -> &CellValue {
        &response
            .calculated_cells()
            .iter()
            .find(|cell| cell.address == address)
            .expect("calculated cell")
            .value
    }

    fn richly_formatted_cell() -> CellFormat {
        CellFormat {
            font: Font {
                family: "Calibri".into(),
                size: 14,
                bold: true,
                italic: true,
                underline: true,
            },
            foreground: Some(Color::new(0x11, 0x22, 0x33, 0xFF)),
            background: Some(Color::new(0xEE, 0xDD, 0xCC, 0xFF)),
            borders: Borders {
                top: Some(Border {
                    style: BorderStyle::Solid,
                    color: Some(Color::new(0xAA, 0xBB, 0xCC, 0xFF)),
                }),
                right: Some(Border {
                    style: BorderStyle::Dashed,
                    color: Some(Color::new(0x12, 0x34, 0x56, 0xFF)),
                }),
                bottom: None,
                left: None,
            },
            alignment: Alignment {
                horizontal: Some(HorizontalAlign::Center),
                vertical: Some(VerticalAlign::Middle),
                wrap: true,
            },
            number_format: NumberFormat::Currency {
                symbol: "€".into(),
                precision: 2,
            },
        }
    }

    #[test]
    fn roundtrips_direct_cell_formats_through_ironcalc() {
        let sheet = SheetId(1);
        let formatted = richly_formatted_cell();
        let snapshot = WorkbookSnapshot {
            revision: Revision(0),
            active_sheet: sheet,
            sheets: vec![SheetSnapshot {
                id: sheet,
                name: "Data".into(),
                cells: vec![
                    CellSnapshot {
                        address: address(sheet, 0, 0),
                        input: CellInput::Number(42.0),
                        format: Some(formatted.clone()),
                    },
                    CellSnapshot {
                        address: address(sheet, 0, 1),
                        input: CellInput::Empty,
                        format: Some(formatted.clone()),
                    },
                ],
                ..SheetSnapshot::default()
            }],
        };

        let model = model_from_snapshot(&snapshot).expect("model");
        let round_tripped = snapshot_from_model(&model).expect("snapshot");
        let cells = &round_tripped.sheets[0].cells;
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].input, CellInput::Number(42.0));
        assert_eq!(cells[0].format.as_ref(), Some(&formatted));
        assert_eq!(cells[1].input, CellInput::Empty);
        assert_eq!(cells[1].format.as_ref(), Some(&formatted));
    }

    #[test]
    fn snapshot_imports_hidden_column_ranges() {
        let mut model = Model::new_empty(
            ENGINE_WORKBOOK_NAME,
            ENGINE_LOCALE,
            ENGINE_TIMEZONE,
            ENGINE_LANGUAGE,
        )
        .expect("model");
        model.set_column_width(0, 2, 126.0).expect("width B");
        model.set_column_hidden(0, 2, true).expect("hide B");
        model.set_column_hidden(0, 3, true).expect("hide C");

        let snapshot = snapshot_from_model(&model).expect("snapshot");
        assert_eq!(
            snapshot.sheets[0].manual_hidden_columns,
            vec![ColumnIndex(1), ColumnIndex(2)]
        );
        assert_eq!(
            snapshot.sheets[0].column_dimensions,
            vec![ColumnDimension {
                column: ColumnIndex(1),
                width: 126.0,
            }]
        );
    }

    #[test]
    fn evaluates_with_real_ironcalc_engine() {
        let sheet = SheetId(1);
        let a1 = address(sheet, 0, 0);
        let b1 = address(sheet, 0, 1);
        let c1 = address(sheet, 0, 2);
        let request = request_with(&[
            (a1, CellInput::Number(2.0)),
            (b1, CellInput::Number(4.0)),
            (
                c1,
                formula("SUM($A$1:B1)+AVERAGE(A1:B1)+MIN(A1:B1)+MAX(A1:B1)"),
            ),
        ]);
        let mut adapter = IroncalcAdapter::new();
        let response = adapter.evaluate(request).expect("evaluation");
        assert_eq!(value_for(&response, c1), &CellValue::Number(15.0));
        assert_eq!(adapter.backend_name(), "ironcalc-0.8.3");
    }

    #[test]
    fn evaluates_cross_sheet_formula_through_ironcalc() {
        let mut workbook = Workbook::new();
        let primary = workbook.active_sheet();
        let mut add_sheet = ChangeSet::new();
        add_sheet.push(Change::AddSheet {
            name: Some("Data".into()),
        });
        let mut plan = workbook.prepare(add_sheet).expect("add sheet");
        workbook
            .commit(
                &mut plan,
                EngineResponse::new(workbook.revision(), Vec::new()),
            )
            .expect("commit sheet");
        let data = workbook.active_sheet();
        let source = address(data, 0, 0);
        let target = address(primary, 0, 0);
        let mut changes = ChangeSet::new();
        changes.push(Change::SetCellInput {
            sheet: data,
            address: source,
            input: CellInput::Number(41.0),
        });
        changes.push(Change::SetCellInput {
            sheet: primary,
            address: target,
            input: formula("Data!A1+1"),
        });
        let request = workbook
            .prepare(changes)
            .expect("valid changes")
            .engine_request();
        let response = IroncalcAdapter::new()
            .evaluate(request)
            .expect("evaluation");
        assert_eq!(value_for(&response, target), &CellValue::Number(42.0));
    }

    #[test]
    fn maps_ironcalc_errors_to_core_errors() {
        let sheet = SheetId(1);
        let div_zero = address(sheet, 0, 0);
        let unknown = address(sheet, 0, 1);
        let missing = address(sheet, 0, 2);
        let response = IroncalcAdapter::new()
            .evaluate(request_with(&[
                (div_zero, formula("1/0")),
                (unknown, formula("UNKNOWN(1)")),
                (missing, formula("#REF!")),
            ]))
            .expect("evaluation");
        assert_eq!(
            value_for(&response, div_zero),
            &CellValue::Error(CellError::Div0)
        );
        assert_eq!(
            value_for(&response, unknown),
            &CellValue::Error(CellError::Name)
        );
        assert_eq!(
            value_for(&response, missing),
            &CellValue::Error(CellError::Ref)
        );
    }

    #[test]
    fn core_number_format_parses_supported_percent_formats() {
        assert_eq!(core_number_format("0%"), NumberFormat::Percent(0));
        assert_eq!(core_number_format("0.0%"), NumberFormat::Percent(1));
        assert_eq!(core_number_format("0.00%"), NumberFormat::Percent(2));
        assert_eq!(core_number_format("0.000%"), NumberFormat::Percent(3));
    }

    #[test]
    fn core_number_format_rejects_unsupported_percent_suffixes() {
        assert_eq!(
            core_number_format("10%"),
            NumberFormat::Custom("10%".to_string())
        );
        assert_eq!(
            core_number_format("0.1%"),
            NumberFormat::Custom("0.1%".to_string())
        );
        assert_eq!(
            core_number_format("abc%"),
            NumberFormat::Custom("abc%".to_string())
        );
    }

    #[test]
    fn native_display_formats_decimal_currency_and_percent() {
        assert_eq!(
            formatted_numeric_display(&CellValue::Number(20.0), &NumberFormat::Decimal(2)),
            Some("20.00".to_string())
        );
        assert_eq!(
            formatted_numeric_display(
                &CellValue::Number(20.0),
                &NumberFormat::Currency {
                    symbol: "$".to_string(),
                    precision: 2,
                },
            ),
            Some("$20.00".to_string())
        );
        assert_eq!(
            formatted_numeric_display(&CellValue::Percent(0.125), &NumberFormat::Percent(2)),
            Some("12.50%".to_string())
        );
    }

    #[test]
    fn native_display_formats_date_and_time_serials() {
        assert_eq!(
            formatted_numeric_display(&CellValue::Date(46015.0), &NumberFormat::Date),
            Some("2025-12-24".to_string())
        );
        assert_eq!(
            formatted_numeric_display(&CellValue::Time(0.25), &NumberFormat::Time),
            Some("06:00:00".to_string())
        );
    }

    #[test]
    fn native_display_leaves_general_nonnumeric_and_invalid_format_to_caller() {
        assert_eq!(
            formatted_numeric_display(&CellValue::Number(20.0), &NumberFormat::General),
            None
        );
        assert_eq!(
            formatted_numeric_display(
                &CellValue::Text("20".to_string()),
                &NumberFormat::Decimal(2)
            ),
            None
        );
        assert_eq!(
            formatted_numeric_display(
                &CellValue::Error(CellError::Div0),
                &NumberFormat::Decimal(2)
            ),
            None
        );
        assert_eq!(
            formatted_numeric_display(
                &CellValue::Number(20.0),
                &NumberFormat::Custom("[".to_string())
            ),
            None
        );
        assert_eq!(
            formatted_numeric_display(&CellValue::Number(f64::NAN), &NumberFormat::Decimal(2)),
            None
        );
    }
}
