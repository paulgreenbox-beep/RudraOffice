//! XLSX package boundary backed by the local IronCalc ZIP/XML implementation.
//!
//! The generic package traits remain available for alternate backends, while
//! [`IroncalcXlsxParts`] is the production codec used by Sheets.

use std::error::Error;
use std::fmt;
use std::io::Cursor;

use ironcalc::{base::Model, export::save_xlsx_to_writer, import::load_from_xlsx_bytes};
use sheets_core::{
    CellInput, CellRange, WorkbookSnapshot, is_same_sheet_name,
    validate_sheet_name as core_validate_sheet_name,
};
use sheets_ironcalc_adapter::{model_from_snapshot, snapshot_from_model};

mod bounded;

pub use bounded::{
    BOUNDED_CELLS_PER_SHEET, BOUNDED_WINDOW_COLUMNS, BOUNDED_WINDOW_ROWS, BoundedSheetWindow,
    import_bounded_sheet_window,
};

/// Errors returned by the XLSX boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum XlsxError {
    /// The requested operation needs a ZIP/XML implementation that is not
    /// present in this crate.
    Unsupported(String),
    /// The core snapshot cannot be represented safely by the boundary.
    InvalidSnapshot(String),
    /// A package source could not provide its bytes.
    PackageRead(String),
    /// A package sink could not accept the complete byte sequence.
    PackageWrite(String),
    /// A ZIP/XML parts implementation rejected the input or output.
    Parts(String),
}

impl XlsxError {
    fn unsupported_backend() -> Self {
        Self::Unsupported("ZIP/XML backend is unavailable".into())
    }
}

impl fmt::Display for XlsxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message) => write!(f, "unsupported XLSX operation: {message}"),
            Self::InvalidSnapshot(message) => write!(f, "invalid workbook snapshot: {message}"),
            Self::PackageRead(message) => write!(f, "XLSX package read failed: {message}"),
            Self::PackageWrite(message) => write!(f, "XLSX package write failed: {message}"),
            Self::Parts(message) => write!(f, "XLSX parts codec failed: {message}"),
        }
    }
}

impl Error for XlsxError {}

/// Read a complete package byte sequence.
///
/// The default method is intentionally useful for small adapters that prefer
/// the more descriptive [`PackageReader::read_package`] name.  Implementors
/// may override either method; a reader that overrides neither reports an
/// unsupported operation.
pub trait PackageReader {
    /// Reads the complete package.
    fn read(&mut self) -> Result<Vec<u8>, XlsxError> {
        self.read_package()
    }

    /// Alias for [`PackageReader::read`].
    fn read_package(&mut self) -> Result<Vec<u8>, XlsxError> {
        Err(XlsxError::PackageRead(
            "package reader has no read implementation".into(),
        ))
    }
}

/// Write one complete package byte sequence.
///
/// The caller constructs all bytes before invoking this method.  A concrete
/// sink is therefore responsible for making this one write atomic with
/// respect to its destination; the boundary never reports success after an
/// error from the sink.
pub trait PackageWriter {
    /// Writes the complete package.
    fn write(&mut self, package: &[u8]) -> Result<(), XlsxError> {
        self.write_package(package)
    }

    /// Alias for [`PackageWriter::write`].
    fn write_package(&mut self, package: &[u8]) -> Result<(), XlsxError> {
        let _ = package;
        Err(XlsxError::PackageWrite(
            "package writer has no write implementation".into(),
        ))
    }
}

/// ZIP/XML-facing codec for the bytes inside an XLSX package.
///
/// A backend owns the actual ZIP and XML work.  The default methods make the
/// common `import`/`export` vocabulary canonical while allowing a backend to
/// use the equivalent `decode`/`encode` names.  No implementation is supplied
/// here because std has no ZIP/XML support.
pub trait XlsxPartsCodec {
    /// Decodes package bytes into the canonical core snapshot.
    fn import(&self, package: &[u8]) -> Result<WorkbookSnapshot, XlsxError> {
        self.decode(package)
    }

    /// Encodes the canonical core snapshot as a complete package.
    fn export(&self, snapshot: &WorkbookSnapshot) -> Result<Vec<u8>, XlsxError> {
        self.encode(snapshot)
    }

    /// Alias for [`XlsxPartsCodec::import`].
    fn decode(&self, package: &[u8]) -> Result<WorkbookSnapshot, XlsxError> {
        let _ = package;
        Err(XlsxError::Parts(
            "parts codec has no decode implementation".into(),
        ))
    }

    /// Alias for [`XlsxPartsCodec::export`].
    fn encode(&self, snapshot: &WorkbookSnapshot) -> Result<Vec<u8>, XlsxError> {
        let _ = snapshot;
        Err(XlsxError::Parts(
            "parts codec has no encode implementation".into(),
        ))
    }
}

/// High-level import/export contract over a package source and sink.
///
/// The trait has no generic methods and is therefore usable as
/// `Box<dyn WorkbookCodec>`.
pub trait WorkbookCodec {
    /// Reads and decodes a complete package.
    fn import(&self, reader: &mut dyn PackageReader) -> Result<WorkbookSnapshot, XlsxError>;

    /// Validates, encodes, and writes a complete package.
    fn export(
        &self,
        snapshot: &WorkbookSnapshot,
        writer: &mut dyn PackageWriter,
    ) -> Result<(), XlsxError>;
}

/// Generic high-level XLSX codec using a supplied ZIP/XML parts backend.
#[derive(Clone, Debug)]
pub struct XlsxCodec<P> {
    parts: P,
}

impl<P> XlsxCodec<P> {
    /// Creates a high-level codec around a parts backend.
    pub const fn new(parts: P) -> Self {
        Self { parts }
    }

    /// Borrows the configured parts backend.
    pub const fn parts(&self) -> &P {
        &self.parts
    }

    /// Consumes the high-level codec and returns its parts backend.
    pub fn into_parts(self) -> P {
        self.parts
    }
}

impl<P: XlsxPartsCodec> XlsxCodec<P> {
    /// Imports through a package reader.
    pub fn import(&self, reader: &mut dyn PackageReader) -> Result<WorkbookSnapshot, XlsxError> {
        <Self as WorkbookCodec>::import(self, reader)
    }

    /// Exports through a package writer.
    pub fn export(
        &self,
        snapshot: &WorkbookSnapshot,
        writer: &mut dyn PackageWriter,
    ) -> Result<(), XlsxError> {
        <Self as WorkbookCodec>::export(self, snapshot, writer)
    }
}

impl<P: XlsxPartsCodec> WorkbookCodec for XlsxCodec<P> {
    fn import(&self, reader: &mut dyn PackageReader) -> Result<WorkbookSnapshot, XlsxError> {
        let package = reader.read()?;
        let snapshot = self.parts.import(&package)?;
        validate_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    fn export(
        &self,
        snapshot: &WorkbookSnapshot,
        writer: &mut dyn PackageWriter,
    ) -> Result<(), XlsxError> {
        // Validation is deliberately before both the parts codec and the
        // writer.  The encoded bytes are also fully materialized before the
        // sink is called, so no package is emitted by a failing codec.
        validate_snapshot(snapshot)?;
        let package = self.parts.export(snapshot)?;
        writer.write(&package)
    }
}

/// Parts backend used until a ZIP/XML implementation is intentionally added.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UnsupportedPartsCodec;

impl XlsxPartsCodec for UnsupportedPartsCodec {
    fn import(&self, package: &[u8]) -> Result<WorkbookSnapshot, XlsxError> {
        let _ = package;
        Err(XlsxError::unsupported_backend())
    }

    fn export(&self, snapshot: &WorkbookSnapshot) -> Result<Vec<u8>, XlsxError> {
        let _ = snapshot;
        Err(XlsxError::unsupported_backend())
    }
}

/// Returns the explicit, non-functional codec used when no ZIP/XML backend
/// is available.
pub fn unavailable_codec() -> XlsxCodec<UnsupportedPartsCodec> {
    XlsxCodec::new(UnsupportedPartsCodec)
}

/// IronCalc ZIP/XML implementation used for real XLSX import and export.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IroncalcXlsxParts;

impl XlsxPartsCodec for IroncalcXlsxParts {
    fn import(&self, package: &[u8]) -> Result<WorkbookSnapshot, XlsxError> {
        if bounded::should_use_bounded_import(package) {
            return bounded::import_bounded_snapshot(package);
        }
        let workbook = load_from_xlsx_bytes(package, "RudraOffice", "en", "UTC")
            .map_err(|error| XlsxError::Parts(format!("IronCalc import failed: {error}")))?;
        let model = Model::from_workbook(workbook, "en").map_err(|error| {
            XlsxError::Parts(format!("IronCalc model creation failed: {error}"))
        })?;
        snapshot_from_model(&model).map_err(|error| {
            XlsxError::Parts(format!("IronCalc snapshot conversion failed: {error}"))
        })
    }

    fn export(&self, snapshot: &WorkbookSnapshot) -> Result<Vec<u8>, XlsxError> {
        let mut model = model_from_snapshot(snapshot).map_err(|error| {
            XlsxError::Parts(format!("IronCalc snapshot conversion failed: {error}"))
        })?;
        model.evaluate();
        save_xlsx_to_writer(&model, Cursor::new(Vec::new()))
            .map(Cursor::into_inner)
            .map_err(|error| XlsxError::Parts(format!("IronCalc export failed: {error}")))
    }
}

/// Returns whether the package uses the bounded large-workbook viewer import.
///
/// Large workbooks are intentionally kept out of the eager IronCalc recalculation
/// path so the first visible grid is not blocked by a full workbook evaluation.
pub fn uses_bounded_import(package: &[u8]) -> bool {
    bounded::should_use_bounded_import(package)
}
/// Returns the production XLSX codec backed by the local IronCalc source tree.
pub fn ironcalc_codec() -> XlsxCodec<IroncalcXlsxParts> {
    XlsxCodec::new(IroncalcXlsxParts)
}
/// Small in-memory complete-package source/sink useful for adapters and tests.
///
/// It stores opaque bytes; it does not claim that those bytes are a valid XLSX
/// package.  The ZIP/XML meaning remains the responsibility of
/// [`XlsxPartsCodec`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MemoryPackage {
    bytes: Vec<u8>,
}

impl MemoryPackage {
    /// Creates an empty in-memory package.
    pub const fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    /// Creates an in-memory package containing `bytes`.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }

    /// Returns the currently stored bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns an owned copy of the currently stored bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl PackageReader for MemoryPackage {
    fn read(&mut self) -> Result<Vec<u8>, XlsxError> {
        Ok(self.bytes.clone())
    }
}

impl PackageWriter for MemoryPackage {
    fn write(&mut self, package: &[u8]) -> Result<(), XlsxError> {
        self.bytes.clear();
        self.bytes.extend_from_slice(package);
        Ok(())
    }
}

fn validate_snapshot(snapshot: &WorkbookSnapshot) -> Result<(), XlsxError> {
    if snapshot.sheets.is_empty() {
        return Err(invalid_snapshot("workbook must contain at least one sheet"));
    }

    if !snapshot
        .sheets
        .iter()
        .any(|sheet| sheet.id == snapshot.active_sheet)
    {
        return Err(invalid_snapshot("active sheet is not present"));
    }

    for (sheet_index, sheet) in snapshot.sheets.iter().enumerate() {
        validate_sheet_name(&sheet.name)?;

        for other in snapshot.sheets.iter().take(sheet_index) {
            if other.id == sheet.id {
                return Err(invalid_snapshot(format!("duplicate sheet id {}", sheet.id)));
            }
            if is_same_sheet_name(&other.name, &sheet.name) {
                return Err(invalid_snapshot(format!(
                    "duplicate sheet name '{}'",
                    sheet.name
                )));
            }
        }

        for (cell_index, cell) in sheet.cells.iter().enumerate() {
            if cell.address.sheet != sheet.id {
                return Err(invalid_snapshot(format!(
                    "cell {} in sheet '{}' has a different sheet id",
                    cell_index, sheet.name
                )));
            }
            validate_cell_input(&cell.input)?;
            if sheet.cells[..cell_index]
                .iter()
                .any(|previous| previous.address == cell.address)
            {
                return Err(invalid_snapshot(format!(
                    "duplicate cell address in sheet '{}'",
                    sheet.name
                )));
            }
        }

        for dimension in &sheet.row_dimensions {
            if !dimension.height.is_finite() || dimension.height <= 0.0 {
                return Err(invalid_snapshot(format!(
                    "row {} in sheet '{}' has a non-positive or non-finite height",
                    dimension.row, sheet.name
                )));
            }
        }
        for (dimension_index, dimension) in sheet.row_dimensions.iter().enumerate() {
            if sheet.row_dimensions[..dimension_index]
                .iter()
                .any(|previous| previous.row == dimension.row)
            {
                return Err(invalid_snapshot(format!(
                    "duplicate row dimension in sheet '{}'",
                    sheet.name
                )));
            }
        }

        for dimension in &sheet.column_dimensions {
            if !dimension.width.is_finite() || dimension.width <= 0.0 {
                return Err(invalid_snapshot(format!(
                    "column {} in sheet '{}' has a non-positive or non-finite width",
                    dimension.column, sheet.name
                )));
            }
        }
        for (dimension_index, dimension) in sheet.column_dimensions.iter().enumerate() {
            if sheet.column_dimensions[..dimension_index]
                .iter()
                .any(|previous| previous.column == dimension.column)
            {
                return Err(invalid_snapshot(format!(
                    "duplicate column dimension in sheet '{}'",
                    sheet.name
                )));
            }
        }

        for (merge_index, merge) in sheet.merges.iter().enumerate() {
            validate_range(merge, sheet.id, &sheet.name)?;
            if sheet.merges[..merge_index]
                .iter()
                .any(|previous| previous.intersects(merge))
            {
                return Err(invalid_snapshot(format!(
                    "overlapping merge in sheet '{}'",
                    sheet.name
                )));
            }
        }
    }

    Ok(())
}

fn validate_sheet_name(name: &str) -> Result<(), XlsxError> {
    core_validate_sheet_name(name).map_err(|core_error| invalid_snapshot(core_error.to_string()))
}

fn validate_cell_input(input: &CellInput) -> Result<(), XlsxError> {
    match input {
        CellInput::Number(value)
        | CellInput::Percent(value)
        | CellInput::Date(value)
        | CellInput::Time(value)
            if !value.is_finite() =>
        {
            Err(invalid_snapshot("cell input contains a non-finite number"))
        }
        CellInput::Formula(formula) if formula.trim() != formula || formula.starts_with('=') => {
            Err(invalid_snapshot(
                "formula must be normalized without leading/trailing whitespace or leading '='",
            ))
        }
        _ => Ok(()),
    }
}

fn validate_range(
    range: &CellRange,
    sheet_id: sheets_core::SheetId,
    sheet_name: &str,
) -> Result<(), XlsxError> {
    if range.sheet != sheet_id || range.start.sheet != sheet_id || range.end.sheet != sheet_id {
        return Err(invalid_snapshot(format!(
            "merge in sheet '{}' has a different sheet id",
            sheet_name
        )));
    }
    if range.start.row.0 > range.end.row.0 || range.start.column.0 > range.end.column.0 {
        return Err(invalid_snapshot(format!(
            "merge in sheet '{}' is not normalized",
            sheet_name
        )));
    }
    Ok(())
}

fn invalid_snapshot(message: impl Into<String>) -> XlsxError {
    XlsxError::InvalidSnapshot(message.into())
}

#[cfg(test)]
mod tests {
    use super::bounded::import_bounded_snapshot;
    use super::*;
    use sheets_core::{
        Alignment, Border, BorderStyle, Borders, CellAddress, CellError, CellFormat, CellInput,
        CellSnapshot, Color, ColumnIndex, Font, HorizontalAlign, NumberFormat, Revision, RowIndex,
        SheetId, SheetSnapshot, VerticalAlign,
    };
    use std::{
        cell::Cell,
        io::{Cursor, Read, Write},
    };

    #[derive(Debug, Default)]
    struct FakeParts {
        imports: Cell<usize>,
        exports: Cell<usize>,
        round_trip: Cell<bool>,
    }

    impl XlsxPartsCodec for FakeParts {
        fn import(&self, package: &[u8]) -> Result<WorkbookSnapshot, XlsxError> {
            self.imports.set(self.imports.get() + 1);
            if package == b"snapshot" {
                Ok(sample_snapshot())
            } else if package == b"encoded" {
                self.round_trip.set(true);
                Ok(sample_snapshot())
            } else {
                Err(XlsxError::Parts("unexpected package".into()))
            }
        }

        fn export(&self, snapshot: &WorkbookSnapshot) -> Result<Vec<u8>, XlsxError> {
            self.exports.set(self.exports.get() + 1);
            assert_eq!(snapshot, &sample_snapshot());
            Ok(b"encoded".to_vec())
        }
    }

    #[derive(Debug, Default)]
    struct CountingParts {
        exports: Cell<usize>,
    }

    impl XlsxPartsCodec for CountingParts {
        fn export(&self, _snapshot: &WorkbookSnapshot) -> Result<Vec<u8>, XlsxError> {
            self.exports.set(self.exports.get() + 1);
            Ok(b"encoded".to_vec())
        }
    }

    #[derive(Debug, Default)]
    struct RecordingWriter {
        calls: usize,
        bytes: Vec<u8>,
    }

    impl PackageWriter for RecordingWriter {
        fn write(&mut self, package: &[u8]) -> Result<(), XlsxError> {
            self.calls += 1;
            self.bytes = package.to_vec();
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FailingWriter {
        calls: usize,
        error: Option<XlsxError>,
    }

    impl PackageWriter for FailingWriter {
        fn write(&mut self, _package: &[u8]) -> Result<(), XlsxError> {
            self.calls += 1;
            Err(self
                .error
                .clone()
                .unwrap_or_else(|| XlsxError::PackageWrite("writer failed".into())))
        }
    }

    #[derive(Debug)]
    struct FailingReader;

    impl PackageReader for FailingReader {
        fn read(&mut self) -> Result<Vec<u8>, XlsxError> {
            Err(XlsxError::PackageRead("reader failed".into()))
        }
    }

    fn address(sheet: SheetId, row: u32, column: u32) -> CellAddress {
        CellAddress::new(sheet, RowIndex(row), ColumnIndex(column))
    }

    fn sample_snapshot() -> WorkbookSnapshot {
        let sheet = SheetId(1);
        WorkbookSnapshot {
            revision: Revision(7),
            active_sheet: sheet,
            sheets: vec![SheetSnapshot {
                id: sheet,
                name: "Data".into(),
                cells: vec![
                    CellSnapshot {
                        address: address(sheet, 0, 0),
                        input: CellInput::Text(String::new()),
                        format: None,
                    },
                    CellSnapshot {
                        address: address(sheet, 0, 1),
                        input: CellInput::Formula("SUM(A1)".into()),
                        format: None,
                    },
                    CellSnapshot {
                        address: address(sheet, 0, 2),
                        input: CellInput::Empty,
                        format: None,
                    },
                ],
                ..SheetSnapshot::default()
            }],
        }
    }

    fn styled_snapshot() -> WorkbookSnapshot {
        let sheet = SheetId(1);
        let format = CellFormat {
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
        };
        WorkbookSnapshot {
            revision: Revision(0),
            active_sheet: sheet,
            sheets: vec![SheetSnapshot {
                id: sheet,
                name: "Styled".into(),
                cells: vec![
                    CellSnapshot {
                        address: address(sheet, 0, 0),
                        input: CellInput::Number(42.0),
                        format: Some(format.clone()),
                    },
                    CellSnapshot {
                        address: address(sheet, 0, 1),
                        input: CellInput::Empty,
                        format: Some(format),
                    },
                ],
                ..SheetSnapshot::default()
            }],
        }
    }
    /// Produces a compact real XLSX package that models Excel's automatic
    /// system foreground/background indices: 64 and 65 respectively.
    ///
    /// The package starts from the production exporter so that the regression
    /// exercises IronCalc parsing plus Rudra's Sheets projection, not a mock
    /// XML reader.  Only the two colour attributes are substituted.
    fn system_indexed_color_fixture() -> Vec<u8> {
        let codec = ironcalc_codec();
        let mut source = MemoryPackage::new();
        codec
            .export(&styled_snapshot(), &mut source)
            .expect("fixture XLSX export");

        let mut archive =
            zip::ZipArchive::new(Cursor::new(source.into_bytes())).expect("fixture source archive");
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::FileOptions::default();
        let mut replaced_font = false;
        let mut replaced_fill = false;

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("fixture source entry");
            let name = entry.name().to_owned();
            if entry.is_dir() {
                output
                    .add_directory(name, options)
                    .expect("fixture directory");
                continue;
            }

            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("fixture entry bytes");
            output.start_file(&name, options).expect("fixture file");
            if name == "xl/styles.xml" {
                let styles = String::from_utf8(bytes).expect("fixture styles UTF-8");
                replaced_font = styles.contains(r#"rgb="FF112233""#);
                let styles = styles.replacen(r#"rgb="FF112233""#, r#"indexed="64""#, 1);
                replaced_fill = styles.contains(r#"rgb="FFEEDDCC""#);
                let styles = styles.replacen(r#"rgb="FFEEDDCC""#, r#"indexed="65""#, 1);
                output
                    .write_all(styles.as_bytes())
                    .expect("fixture styles write");
            } else {
                output.write_all(&bytes).expect("fixture file write");
            }
        }

        assert!(
            replaced_font,
            "fixture must contain the explicit font colour"
        );
        assert!(
            replaced_fill,
            "fixture must contain the explicit fill colour"
        );
        output.finish().expect("fixture archive").into_inner()
    }

    fn manual_hidden_axes_fixture() -> Vec<u8> {
        let codec = ironcalc_codec();
        let mut source = MemoryPackage::new();
        codec
            .export(&sample_snapshot(), &mut source)
            .expect("fixture XLSX export");

        let mut archive =
            zip::ZipArchive::new(Cursor::new(source.into_bytes())).expect("fixture source archive");
        let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::FileOptions::default();
        let mut replaced_sheet = false;

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("fixture source entry");
            let name = entry.name().to_owned();
            if entry.is_dir() {
                output
                    .add_directory(name, options)
                    .expect("fixture directory");
                continue;
            }

            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("fixture entry bytes");
            output.start_file(&name, options).expect("fixture file");
            if name == "xl/worksheets/sheet1.xml" {
                let mut sheet = String::from_utf8(bytes).expect("fixture sheet UTF-8");
                let row = r#"<row r="1""#;
                assert!(sheet.contains(row), "fixture row must be present");
                sheet = sheet.replacen(row, r#"<row r="1" hidden="1""#, 1);

                let hidden_column =
                    r#"<cols><col min="2" max="2" width="18" customWidth="1" hidden="1"/></cols>"#;
                if let Some(columns_start) = sheet.find("<cols") {
                    let columns_end = columns_start
                        + sheet[columns_start..]
                            .find("</cols>")
                            .expect("fixture columns end")
                        + "</cols>".len();
                    sheet.replace_range(columns_start..columns_end, hidden_column);
                } else {
                    let sheet_data = sheet.find("<sheetData").expect("fixture sheet data");
                    sheet.insert_str(sheet_data, hidden_column);
                }
                output
                    .write_all(sheet.as_bytes())
                    .expect("fixture sheet write");
                replaced_sheet = true;
            } else {
                output.write_all(&bytes).expect("fixture file write");
            }
        }

        assert!(replaced_sheet, "fixture must contain the first worksheet");
        output.finish().expect("fixture archive").into_inner()
    }

    #[test]
    fn unavailable_backend_is_stable_and_explicit() {
        let codec = unavailable_codec();
        let snapshot = sample_snapshot();
        let mut writer = MemoryPackage::new();
        let export_error = codec.export(&snapshot, &mut writer).unwrap_err();
        assert!(matches!(export_error, XlsxError::Unsupported(_)));
        assert_eq!(
            export_error.to_string(),
            "unsupported XLSX operation: ZIP/XML backend is unavailable"
        );
        assert!(writer.bytes().is_empty());

        let mut reader = MemoryPackage::from_bytes(b"not-an-xlsx".to_vec());
        let import_error = codec.import(&mut reader).unwrap_err();
        assert_eq!(import_error, export_error);
        assert!(import_error.source().is_none());
    }

    #[test]
    fn xlsx_codec_delegates_import_and_export_to_parts() {
        let parts = FakeParts::default();
        let codec = XlsxCodec::new(parts);
        let mut reader = MemoryPackage::from_bytes(b"snapshot".to_vec());
        assert_eq!(codec.import(&mut reader).unwrap(), sample_snapshot());
        assert_eq!(codec.parts().imports.get(), 1);

        let mut writer = MemoryPackage::new();
        codec.export(&sample_snapshot(), &mut writer).unwrap();
        assert_eq!(codec.parts().exports.get(), 1);
        assert_eq!(writer.bytes(), b"encoded");
    }

    #[test]
    fn memory_package_reader_and_writer_have_complete_package_semantics() {
        let mut package = MemoryPackage::from_bytes(vec![1, 2, 3]);
        assert_eq!(package.read().unwrap(), vec![1, 2, 3]);
        assert_eq!(package.read().unwrap(), vec![1, 2, 3]);
        package.write(&[9, 8]).unwrap();
        assert_eq!(package.bytes(), &[9, 8]);
    }

    #[test]
    fn import_and_export_preserve_snapshot_data_through_fake_parts() {
        let snapshot = sample_snapshot();
        let codec = XlsxCodec::new(FakeParts::default());
        let mut writer = MemoryPackage::new();
        codec.export(&snapshot, &mut writer).unwrap();
        let mut reader = writer;
        assert_eq!(codec.import(&mut reader).unwrap(), snapshot);
        assert!(codec.parts().round_trip.get());

        // CellError is a calculated value in core, not a WorkbookSnapshot
        // field.  This boundary therefore preserves the input snapshot only;
        // calculated errors cannot be serialized without a core contract for
        // them.
        assert_eq!(CellError::Div0.token(), "#DIV/0!");
    }

    #[test]
    fn validation_happens_before_parts_and_writer() {
        let parts = FakeParts::default();
        let codec = XlsxCodec::new(parts);
        let mut invalid = sample_snapshot();
        invalid.sheets[0].cells[0].input = CellInput::Number(f64::NAN);
        let mut writer = FailingWriter::default();

        let error = codec.export(&invalid, &mut writer).unwrap_err();
        assert!(matches!(error, XlsxError::InvalidSnapshot(_)));
        assert_eq!(codec.parts().exports.get(), 0);
        assert_eq!(writer.calls, 0);
    }

    #[test]
    fn unicode_formula_is_accepted_by_snapshot_validation() {
        let parts = CountingParts::default();
        let codec = XlsxCodec::new(parts);
        let mut snapshot = sample_snapshot();
        snapshot.sheets[0].cells[1].input = CellInput::formula("РЎРЈРњРњ(A1)").unwrap();
        let mut writer = RecordingWriter::default();

        codec.export(&snapshot, &mut writer).unwrap();

        assert_eq!(codec.parts().exports.get(), 1);
        assert_eq!(writer.calls, 1);
        assert_eq!(writer.bytes, b"encoded");
    }

    #[test]
    fn noncanonical_direct_formula_is_rejected_before_parts_and_writer() {
        for formula in ["=SUM(A1)", " SUM(A1)", "SUM(A1) "] {
            let parts = CountingParts::default();
            let codec = XlsxCodec::new(parts);
            let mut invalid = sample_snapshot();
            invalid.sheets[0].cells[1].input = CellInput::Formula(formula.into());
            let mut writer = RecordingWriter::default();

            let error = codec.export(&invalid, &mut writer).unwrap_err();

            assert!(matches!(error, XlsxError::InvalidSnapshot(_)));
            assert_eq!(codec.parts().exports.get(), 0, "formula: {formula:?}");
            assert_eq!(writer.calls, 0, "formula: {formula:?}");
        }
    }

    #[test]
    fn writer_error_is_not_reported_as_partial_success() {
        let codec = XlsxCodec::new(FakeParts::default());
        let mut writer = FailingWriter {
            calls: 0,
            error: Some(XlsxError::PackageWrite("disk full".into())),
        };
        let error = codec.export(&sample_snapshot(), &mut writer).unwrap_err();
        assert_eq!(error, XlsxError::PackageWrite("disk full".into()));
        assert_eq!(writer.calls, 1);
    }

    #[test]
    fn parts_error_is_propagated_without_writer_call() {
        #[derive(Debug)]
        struct FailingParts;
        impl XlsxPartsCodec for FailingParts {
            fn export(&self, _snapshot: &WorkbookSnapshot) -> Result<Vec<u8>, XlsxError> {
                Err(XlsxError::Parts("xml rejected".into()))
            }
        }

        let codec = XlsxCodec::new(FailingParts);
        let mut writer = FailingWriter::default();
        let error = codec.export(&sample_snapshot(), &mut writer).unwrap_err();
        assert_eq!(error, XlsxError::Parts("xml rejected".into()));
        assert_eq!(writer.calls, 0);
    }

    #[test]
    fn reader_error_is_propagated_without_parts_call() {
        let parts = FakeParts::default();
        let codec = XlsxCodec::new(parts);
        let mut reader = FailingReader;
        let error = codec.import(&mut reader).unwrap_err();
        assert_eq!(error, XlsxError::PackageRead("reader failed".into()));
        assert_eq!(codec.parts().imports.get(), 0);
    }

    #[test]
    fn ironcalc_codec_writes_and_reads_a_real_xlsx_package() {
        let codec = ironcalc_codec();
        let mut writer = MemoryPackage::new();
        codec
            .export(&sample_snapshot(), &mut writer)
            .expect("XLSX export");
        assert!(writer.bytes().starts_with(b"PK\x03\x04"));

        let mut reader = MemoryPackage::from_bytes(writer.into_bytes());
        let imported = codec.import(&mut reader).expect("XLSX import");
        assert_eq!(imported.sheets.len(), 1);
        assert_eq!(imported.sheets[0].name, "Data");
        assert!(
            imported.sheets[0]
                .cells
                .iter()
                .any(|cell| matches!(cell.input, CellInput::Formula(_)))
        );
    }

    #[test]
    fn ironcalc_codec_imports_manual_hidden_rows_and_columns() {
        let codec = ironcalc_codec();
        let mut reader = MemoryPackage::from_bytes(manual_hidden_axes_fixture());

        let imported = codec.import(&mut reader).expect("XLSX import");
        assert_eq!(imported.sheets.len(), 1);
        assert_eq!(imported.sheets[0].manual_hidden_rows, vec![RowIndex(0)]);
        assert_eq!(
            imported.sheets[0].manual_hidden_columns,
            vec![ColumnIndex(1)]
        );
    }

    #[test]
    fn ironcalc_codec_roundtrips_supported_xlsx_cell_formatting() {
        let snapshot = styled_snapshot();
        let codec = ironcalc_codec();
        let mut writer = MemoryPackage::new();
        codec.export(&snapshot, &mut writer).expect("XLSX export");
        assert!(writer.bytes().starts_with(b"PK\x03\x04"));

        let mut reader = MemoryPackage::from_bytes(writer.into_bytes());
        let imported = codec.import(&mut reader).expect("XLSX import");
        assert_eq!(imported.sheets[0].cells, snapshot.sheets[0].cells);
    }
    #[test]
    fn ironcalc_codec_keeps_system_indexed_colors_automatic() {
        let codec = ironcalc_codec();
        let mut reader = MemoryPackage::from_bytes(system_indexed_color_fixture());
        let imported = codec
            .import(&mut reader)
            .expect("system-colour XLSX import");
        let cell = imported.sheets[0]
            .cells
            .iter()
            .find(|cell| cell.address == address(SheetId(1), 0, 0))
            .expect("styled fixture cell");
        let format = cell.format.as_ref().expect("styled fixture format");

        // 64/65 are host-system colours, not portable RGB entries. The viewer
        // therefore falls back to its normal foreground/background instead of
        // turning ordinary cells into black-on-black content.
        assert_eq!(format.foreground, None);
        assert_eq!(format.background, None);
    }
    #[test]
    fn bounded_window_reaches_later_rows_on_a_selected_sheet() {
        let first = SheetId(1);
        let second = SheetId(2);
        let snapshot = WorkbookSnapshot {
            revision: Revision(0),
            active_sheet: first,
            sheets: vec![
                SheetSnapshot {
                    id: first,
                    name: "First".into(),
                    cells: vec![CellSnapshot {
                        address: address(first, 0, 0),
                        input: CellInput::Text("initial".into()),
                        format: None,
                    }],
                    ..SheetSnapshot::default()
                },
                SheetSnapshot {
                    id: second,
                    name: "Second".into(),
                    cells: vec![CellSnapshot {
                        address: address(second, 10_000, 3),
                        input: CellInput::Text("later region".into()),
                        format: None,
                    }],
                    ..SheetSnapshot::default()
                },
            ],
        };
        let codec = ironcalc_codec();
        let mut package = MemoryPackage::new();
        codec
            .export(&snapshot, &mut package)
            .expect("fixture XLSX export");

        let imported = import_bounded_sheet_window(
            package.bytes(),
            BoundedSheetWindow::new(1, 9_984, 0, 64, 16, BOUNDED_CELLS_PER_SHEET),
        )
        .expect("later bounded region import");

        assert_eq!(imported.active_sheet, second);
        assert_eq!(imported.sheets.len(), 2);
        assert!(imported.sheets[0].cells.is_empty());
        assert_eq!(
            imported.sheets[1].cells,
            vec![CellSnapshot {
                address: address(second, 10_000, 3),
                input: CellInput::Text("later region".into()),
                format: None,
            }]
        );
    }

    #[test]
    fn bounded_default_window_materializes_its_entire_dense_rectangle() {
        let sheet = SheetId(1);
        let mut cells = Vec::with_capacity(BOUNDED_CELLS_PER_SHEET);
        for row in 0..BOUNDED_WINDOW_ROWS {
            for column in 0..BOUNDED_WINDOW_COLUMNS {
                cells.push(CellSnapshot {
                    address: address(sheet, row, column),
                    input: CellInput::Number(f64::from(row * BOUNDED_WINDOW_COLUMNS + column)),
                    format: None,
                });
            }
        }
        let snapshot = WorkbookSnapshot {
            revision: Revision(0),
            active_sheet: sheet,
            sheets: vec![SheetSnapshot {
                id: sheet,
                name: "Dense".into(),
                cells,
                ..SheetSnapshot::default()
            }],
        };
        let codec = ironcalc_codec();
        let mut package = MemoryPackage::new();
        codec
            .export(&snapshot, &mut package)
            .expect("dense XLSX fixture export");

        let imported =
            import_bounded_snapshot(package.bytes()).expect("default bounded window import");
        assert_eq!(imported.sheets[0].cells.len(), BOUNDED_CELLS_PER_SHEET);
        assert!(imported.sheets[0].cells.iter().any(|cell| {
            cell.address == address(sheet, BOUNDED_WINDOW_ROWS - 1, BOUNDED_WINDOW_COLUMNS - 1)
        }));
    }

    #[test]
    fn bounded_window_rejects_a_rectangle_larger_than_its_cell_budget() {
        let error = import_bounded_sheet_window(
            b"not-a-zip",
            BoundedSheetWindow::new(0, 0, 0, 65, 64, BOUNDED_CELLS_PER_SHEET),
        )
        .expect_err("an incomplete rectangle must not be advertised as resident");
        assert!(
            error
                .to_string()
                .contains("rectangle no larger than its cell limit")
        );
    }

    #[test]
    fn workbook_codec_is_object_safe() {
        let codec: Box<dyn WorkbookCodec> = Box::new(XlsxCodec::new(FakeParts::default()));
        let mut writer = MemoryPackage::new();
        codec.export(&sample_snapshot(), &mut writer).unwrap();
        assert_eq!(writer.bytes(), b"encoded");
    }

    #[test]
    fn xlsx_validates_unicode_scalar_length_and_forbidden_characters() {
        let codec = XlsxCodec::new(CountingParts::default());
        let invalid_name_31 = "\u{0410}".repeat(31);
        assert_eq!(invalid_name_31.chars().count(), 31);
        let mut snapshot = sample_snapshot();
        snapshot.sheets[0].name = format!("{invalid_name_31}\u{0410}");
        let mut writer = RecordingWriter::default();
        assert!(matches!(
            codec.export(&snapshot, &mut writer).unwrap_err(),
            XlsxError::InvalidSnapshot(_)
        ));
        assert_eq!(codec.parts().exports.get(), 0);
        assert_eq!(writer.calls, 0);

        let mut valid_snapshot = sample_snapshot();
        valid_snapshot.sheets[0].name =
            "\u{1F600}\u{1F601}\u{1F602}\u{1F923}\u{1F603}\u{1F604}\u{1F605}\u{1F606}".into();
        assert_eq!(valid_snapshot.sheets[0].name.chars().count(), 8);
        let mut valid_writer = MemoryPackage::new();
        codec.export(&valid_snapshot, &mut valid_writer).unwrap();
        assert_eq!(codec.parts().exports.get(), 1);
    }

    #[test]
    fn xlsx_validates_case_insensitive_duplicate_sheet_names() {
        let codec = XlsxCodec::new(FakeParts::default());
        let mut snapshot = sample_snapshot();
        snapshot.sheets.push(SheetSnapshot {
            id: SheetId(2),
            name: "data".into(),
            cells: vec![],
            merges: vec![],
            row_dimensions: vec![],
            column_dimensions: vec![],
            auto_filter: None,
            manual_hidden_rows: Vec::new(),
            manual_hidden_columns: Vec::new(),
        });
        snapshot.active_sheet = SheetId(2);
        let mut writer = RecordingWriter::default();
        assert!(matches!(
            codec.export(&snapshot, &mut writer).unwrap_err(),
            XlsxError::InvalidSnapshot(_)
        ));
        assert_eq!(writer.calls, 0);
    }

    #[test]
    fn xlsx_wraps_core_invalid_sheet_name_errors() {
        let too_long_name = "\u{0410}".repeat(32);
        let assert_wrapped = |name: &str| {
            let core_error = sheets_core::validate_sheet_name(name)
                .unwrap_err()
                .to_string();
            assert_eq!(
                validate_sheet_name(name),
                Err(XlsxError::InvalidSnapshot(core_error))
            );
        };

        assert_wrapped(" \t ");
        assert_wrapped("A:B");
        assert_wrapped(&too_long_name);
    }
}
