//! Reproducible small XLSX inputs for the public RC26 GUI acceptance scripts.
//! Uses the production API/codec; never overwrites an existing artifact.
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use sheets_api::SpreadsheetApi;
use sheets_core::{CellAddress, CellInput, ColumnIndex, RowIndex};

fn fixture(path: &Path, records: &[&[&str]]) -> Result<(), Box<dyn Error>> {
    if path.exists() {
        return Ok(());
    }
    let mut api = SpreadsheetApi::production();
    let sheet = api.document().active_sheet();
    for (row, cells) in records.iter().enumerate() {
        for (column, text) in cells.iter().enumerate() {
            let input = if text.starts_with('=') {
                CellInput::formula(*text)?
            } else if let Ok(number) = text.parse::<f64>() {
                CellInput::Number(number)
            } else {
                CellInput::Text((*text).into())
            };
            api.set_cell_input(
                sheet,
                CellAddress::new(sheet, RowIndex(row as u32), ColumnIndex(column as u32)),
                input,
            )?;
        }
    }
    let bytes = api.export_xlsx()?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    println!("{}: {} bytes", path.display(), bytes.len());
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let directory = std::env::args()
        .nth(1)
        .ok_or("provide a dedicated evidence fixture directory")?;
    let directory = Path::new(&directory);
    fs::create_dir_all(directory)?;
    fixture(
        &directory.join("sort-formulas.xlsx"),
        &[
            &["Name", "Qty", "Price", "Total"],
            &["A", "3", "10", "=B2*C2"],
            &["B", "1", "20", "=B3*C3"],
            &["C", "2", "15", "=B4*C4"],
        ],
    )?;
    fixture(
        &directory.join("filter-records.xlsx"),
        &[
            &["Category", "Qty"],
            &["A", "10"],
            &["B", "5"],
            &["A", "7"],
            &["C", "1"],
            &["B", "12"],
        ],
    )?;
    Ok(())
}
