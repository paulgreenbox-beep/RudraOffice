use std::io::{Cursor, Read, Seek};

use quick_xml::Reader;
use quick_xml::events::Event;
use sheets_core::{
    CellAddress, CellInput, CellSnapshot, ColumnIndex, Revision, RowIndex, SheetId, SheetSnapshot,
    WorkbookSnapshot,
};
use zip::ZipArchive;

pub const BOUNDED_PACKAGE_BYTES: usize = 8 * 1024 * 1024;
/// Maximum materialized cells in one resident large-workbook window.
pub const BOUNDED_CELLS_PER_SHEET: usize = 4_096;

/// Default row span for the first and subsequently requested viewer windows.
pub const BOUNDED_WINDOW_ROWS: u32 = 256;
/// Default column span for the first and subsequently requested viewer windows.
pub const BOUNDED_WINDOW_COLUMNS: u32 = 16;

/// A finite, source-backed rectangle of one worksheet.
///
/// `sheet_index` is zero-based workbook order. Rows and columns are zero-based
/// and the count fields are exclusive spans. A valid rectangle fits entirely in
/// `cell_limit`, so the parser never claims a larger resident range than it can
/// materialize.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedSheetWindow {
    pub sheet_index: usize,
    pub start_row: u32,
    pub start_column: u32,
    pub row_count: u32,
    pub column_count: u32,
    pub cell_limit: usize,
}

impl BoundedSheetWindow {
    pub const fn new(
        sheet_index: usize,
        start_row: u32,
        start_column: u32,
        row_count: u32,
        column_count: u32,
        cell_limit: usize,
    ) -> Self {
        Self {
            sheet_index,
            start_row,
            start_column,
            row_count,
            column_count,
            cell_limit,
        }
    }

    pub const fn initial() -> Self {
        Self::new(
            0,
            0,
            0,
            BOUNDED_WINDOW_ROWS,
            BOUNDED_WINDOW_COLUMNS,
            BOUNDED_CELLS_PER_SHEET,
        )
    }

    pub const fn contains(self, row: u32, column: u32) -> bool {
        let row_end = self.start_row.saturating_add(self.row_count);
        let column_end = self.start_column.saturating_add(self.column_count);
        row >= self.start_row && row < row_end && column >= self.start_column && column < column_end
    }

    fn is_valid(self) -> bool {
        self.row_count != 0
            && self.column_count != 0
            && self.cell_limit != 0
            && (self.row_count as usize)
                .checked_mul(self.column_count as usize)
                .is_some_and(|area| area <= self.cell_limit)
    }
}

pub fn should_use_bounded_import(package: &[u8]) -> bool {
    package.len() >= BOUNDED_PACKAGE_BYTES
}

/// Imports the first visible source-backed window of a large workbook.
///
/// All worksheet names remain available, but only the active window has cells
/// resident. Call [`import_bounded_sheet_window`] to move the materialized
/// window without loading the whole workbook into the live model.
pub fn import_bounded_snapshot(package: &[u8]) -> Result<WorkbookSnapshot, super::XlsxError> {
    import_bounded_sheet_window(package, BoundedSheetWindow::initial())
}

/// Imports exactly one bounded rectangle from a selected worksheet.
///
/// This is intentionally a source-backed projection, not an eager workbook
/// import: the output preserves sheet identity/order while materializing cells
/// only for `window.sheet_index` and its requested rectangle.
pub fn import_bounded_sheet_window(
    package: &[u8],
    window: BoundedSheetWindow,
) -> Result<WorkbookSnapshot, super::XlsxError> {
    if !window.is_valid() {
        return Err(parts_error(
            "bounded window must have non-zero spans and a rectangle no larger than its cell limit",
        ));
    }

    let mut archive = ZipArchive::new(Cursor::new(package)).map_err(zip_error)?;
    let shared = read_shared_strings(&mut archive)?;
    let names = read_sheet_names(&mut archive)?;
    if names.is_empty() {
        return Err(parts_error("workbook contains no worksheets"));
    }
    if window.sheet_index >= names.len() {
        return Err(parts_error("bounded window sheet index is out of range"));
    }

    let active_sheet = SheetId(window.sheet_index as u32 + 1);
    let mut sheets = Vec::with_capacity(names.len());
    for (index, name) in names.into_iter().enumerate() {
        let sheet_id = SheetId(index as u32 + 1);
        let cells = if index == window.sheet_index {
            let mut xml = Vec::new();
            let entry_name = format!("xl/worksheets/sheet{}.xml", index + 1);
            archive
                .by_name(&entry_name)
                .map_err(zip_error)?
                .read_to_end(&mut xml)
                .map_err(|error| parts_error(error.to_string()))?;
            parse_sheet_cells(&xml, sheet_id, &shared, window)?
        } else {
            Vec::new()
        };
        sheets.push(SheetSnapshot {
            id: sheet_id,
            name,
            cells,
            ..Default::default()
        });
    }
    Ok(WorkbookSnapshot {
        revision: Revision(0),
        active_sheet,
        sheets,
    })
}

fn read_shared_strings<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<String>, super::XlsxError> {
    let mut xml = Vec::new();
    let Ok(mut entry) = archive.by_name("xl/sharedStrings.xml") else {
        return Ok(Vec::new());
    };
    entry
        .read_to_end(&mut xml)
        .map_err(|error| parts_error(error.to_string()))?;
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut buffer = Vec::new();
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_item = false;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) if event.name().as_ref() == b"si" => {
                in_item = true;
                current.clear();
            }
            Ok(Event::Text(event)) if in_item => {
                current.push_str(&String::from_utf8_lossy(event.as_ref()))
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"si" => {
                values.push(std::mem::take(&mut current));
                in_item = false;
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(xml_error(error)),
            _ => {}
        }
        buffer.clear();
    }
    Ok(values)
}

fn read_sheet_names<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<String>, super::XlsxError> {
    let mut xml = Vec::new();
    archive
        .by_name("xl/workbook.xml")
        .map_err(zip_error)?
        .read_to_end(&mut xml)
        .map_err(|error| parts_error(error.to_string()))?;
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut buffer = Vec::new();
    let mut names = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Empty(event)) | Ok(Event::Start(event))
                if event.name().as_ref() == b"sheet" =>
            {
                let name = event
                    .attributes()
                    .flatten()
                    .find(|attribute| attribute.key.as_ref() == b"name")
                    .map(|attribute| attribute.unescape_value().map(|value| value.into_owned()))
                    .transpose()
                    .map_err(xml_error)?
                    .unwrap_or_else(|| "Sheet".to_owned());
                names.push(name);
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(xml_error(error)),
            _ => {}
        }
        buffer.clear();
    }
    Ok(names)
}

fn parse_sheet_cells(
    xml: &[u8],
    sheet: SheetId,
    shared: &[String],
    window: BoundedSheetWindow,
) -> Result<Vec<CellSnapshot>, super::XlsxError> {
    let mut reader = Reader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut cells = Vec::with_capacity(window.cell_limit.min(BOUNDED_CELLS_PER_SHEET));
    let mut reference = None;
    let mut kind = None;
    let mut value = String::new();
    let mut formula = String::new();
    let mut text = String::new();
    let mut field = None;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) if event.name().as_ref() == b"c" => {
                reference = event
                    .attributes()
                    .flatten()
                    .find(|a| a.key.as_ref() == b"r")
                    .map(|a| a.unescape_value().map(|value| value.into_owned()))
                    .transpose()
                    .map_err(xml_error)?;
                kind = event
                    .attributes()
                    .flatten()
                    .find(|a| a.key.as_ref() == b"t")
                    .map(|a| a.unescape_value().map(|value| value.into_owned()))
                    .transpose()
                    .map_err(xml_error)?;
                value.clear();
                formula.clear();
                text.clear();
            }
            Ok(Event::Start(event)) if event.name().as_ref() == b"v" => field = Some(b'v'),
            Ok(Event::Start(event)) if event.name().as_ref() == b"f" => field = Some(b'f'),
            Ok(Event::Start(event)) if event.name().as_ref() == b"t" => field = Some(b't'),
            Ok(Event::Text(event)) => {
                let decoded = String::from_utf8_lossy(event.as_ref());
                match field {
                    Some(b'v') => value.push_str(&decoded),
                    Some(b'f') => formula.push_str(&decoded),
                    Some(b't') => text.push_str(&decoded),
                    _ => {}
                }
            }
            Ok(Event::End(event)) if matches!(event.name().as_ref(), b"v" | b"f" | b"t") => {
                field = None
            }
            Ok(Event::End(event)) if event.name().as_ref() == b"c" => {
                if let Some(reference) = reference.as_deref()
                    && let Some((row, column)) = parse_cell_reference(reference)
                    && window.contains(row, column)
                    && cells.len() < window.cell_limit
                {
                    let input = cell_input(kind.as_deref(), &value, &formula, &text, shared);
                    if !matches!(input, CellInput::Empty) {
                        cells.push(CellSnapshot {
                            address: CellAddress::new(sheet, RowIndex(row), ColumnIndex(column)),
                            input,
                            format: None,
                        });
                    }
                }
                reference = None;
                kind = None;
                field = None;
                if cells.len() >= window.cell_limit {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(xml_error(error)),
            _ => {}
        }
        buffer.clear();
    }
    Ok(cells)
}

fn cell_input(
    kind: Option<&str>,
    value: &str,
    formula: &str,
    text: &str,
    shared: &[String],
) -> CellInput {
    if !formula.trim().is_empty() {
        return CellInput::Formula(formula.trim().to_owned());
    }
    match kind {
        Some("s") => value
            .parse::<usize>()
            .ok()
            .and_then(|index| shared.get(index).cloned())
            .map(CellInput::Text)
            .unwrap_or_else(|| CellInput::Text(value.to_owned())),
        Some("inlineStr") => CellInput::Text(text.to_owned()),
        Some("b") => CellInput::Boolean(value == "1" || value.eq_ignore_ascii_case("true")),
        Some("str") | Some("e") => CellInput::Text(if text.is_empty() {
            value.to_owned()
        } else {
            text.to_owned()
        }),
        _ => value
            .parse::<f64>()
            .map(CellInput::Number)
            .unwrap_or_else(|_| {
                CellInput::Text(if text.is_empty() {
                    value.to_owned()
                } else {
                    text.to_owned()
                })
            }),
    }
}

fn parse_cell_reference(reference: &str) -> Option<(u32, u32)> {
    let split = reference.find(|character: char| character.is_ascii_digit())?;
    let (letters, digits) = reference.split_at(split);
    let mut column = 0u32;
    for character in letters.chars() {
        if !character.is_ascii_alphabetic() {
            return None;
        }
        column = column
            .checked_mul(26)?
            .checked_add(character.to_ascii_uppercase() as u32 - 'A' as u32 + 1)?;
    }
    Some((
        digits.parse::<u32>().ok()?.checked_sub(1)?,
        column.checked_sub(1)?,
    ))
}

fn parts_error(message: impl Into<String>) -> super::XlsxError {
    super::XlsxError::Parts(message.into())
}
fn zip_error(error: zip::result::ZipError) -> super::XlsxError {
    parts_error(format!("ZIP package error: {error}"))
}
fn xml_error(error: impl std::fmt::Display) -> super::XlsxError {
    parts_error(format!("XML parse error: {error}"))
}
