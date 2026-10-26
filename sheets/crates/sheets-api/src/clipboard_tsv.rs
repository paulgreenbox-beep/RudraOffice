//! Bounded plain-text TSV parsing and serialization for the Sheets clipboard.
//!
//! This module only handles the text matrix. Callers decide how cell strings
//! become inputs and which displayed values are supplied for export.

use crate::{ApiResult, ApiUnsupportedError};

/// Maximum UTF-8 size accepted for either parsed or serialized TSV text.
pub const MAX_CLIPBOARD_TSV_BYTES: usize = 1024 * 1024;

/// Maximum rectangular cell count accepted by the clipboard TSV helpers.
pub const MAX_CLIPBOARD_CELLS: usize = 10_000;

const MAX_CLIPBOARD_ROWS: usize = MAX_CLIPBOARD_CELLS;
const MAX_CLIPBOARD_COLUMNS: usize = MAX_CLIPBOARD_CELLS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Terminator {
    Field,
    Record,
    End,
}

/// Parses bounded TSV into a rectangular matrix, padding short rows with blanks.
///
/// A final record separator terminates the preceding row without creating a
/// synthetic row after it. An empty input is represented as one blank cell.
pub(crate) fn parse_tsv(text: &str) -> ApiResult<Vec<Vec<String>>> {
    if text.len() > MAX_CLIPBOARD_TSV_BYTES {
        return Err(clipboard_error(format!(
            "clipboard TSV exceeds the {MAX_CLIPBOARD_TSV_BYTES}-byte limit"
        )));
    }
    if text.is_empty() {
        return Ok(vec![vec![String::new()]]);
    }

    let bytes = text.as_bytes();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut max_columns = 0;
    let mut index = 0;
    let mut field_pending = true;

    while index < bytes.len() || field_pending {
        field_pending = false;
        check_next_field(rows.len(), row.len(), max_columns)?;
        let (field, terminator) = parse_field(text, &mut index)?;
        row.push(field);

        match terminator {
            Terminator::Field => field_pending = true,
            Terminator::Record => {
                finish_row(&mut rows, &mut row, &mut max_columns)?;
                // Do not synthesize another row when the separator was final.
                if index == bytes.len() {
                    break;
                }
            }
            Terminator::End => {
                finish_row(&mut rows, &mut row, &mut max_columns)?;
                break;
            }
        }
    }

    let rectangular_cells = rows
        .len()
        .checked_mul(max_columns)
        .ok_or_else(|| clipboard_error("clipboard TSV dimensions overflow"))?;
    if rectangular_cells > MAX_CLIPBOARD_CELLS {
        return Err(clipboard_error(format!(
            "clipboard TSV exceeds the {MAX_CLIPBOARD_CELLS}-cell limit"
        )));
    }

    for row in &mut rows {
        row.resize_with(max_columns, String::new);
    }
    Ok(rows)
}

/// Serializes a matrix as quoted TSV, using CRLF record separators.
///
/// Ragged rows are padded with blank fields. Every output ends in CRLF; the
/// parser treats that final separator as the end of the last row.
pub(crate) fn serialize_tsv(rows: &[Vec<String>]) -> ApiResult<String> {
    if rows.is_empty() {
        return Err(clipboard_error("clipboard TSV requires at least one row"));
    }
    if rows.len() > MAX_CLIPBOARD_ROWS {
        return Err(clipboard_error(format!(
            "clipboard TSV exceeds the {MAX_CLIPBOARD_ROWS}-row limit"
        )));
    }

    let columns = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    if columns > MAX_CLIPBOARD_COLUMNS {
        return Err(clipboard_error(format!(
            "clipboard TSV exceeds the {MAX_CLIPBOARD_COLUMNS}-column limit"
        )));
    }
    let rectangular_cells = rows
        .len()
        .checked_mul(columns)
        .ok_or_else(|| clipboard_error("clipboard TSV dimensions overflow"))?;
    if rectangular_cells > MAX_CLIPBOARD_CELLS {
        return Err(clipboard_error(format!(
            "clipboard TSV exceeds the {MAX_CLIPBOARD_CELLS}-cell limit"
        )));
    }

    // Count the complete result before allocating or growing its String.
    let mut output_bytes = 0usize;
    for row in rows {
        for column in 0..columns {
            let value = row.get(column).map(String::as_str).unwrap_or("");
            add_output_bytes(&mut output_bytes, serialized_field_len(value)?)?;
            if column + 1 < columns {
                add_output_bytes(&mut output_bytes, 1)?;
            }
        }
        add_output_bytes(&mut output_bytes, 2)?;
    }

    let mut output = String::with_capacity(output_bytes);
    for row in rows {
        for column in 0..columns {
            if column > 0 {
                output.push('\t');
            }
            let value = row.get(column).map(String::as_str).unwrap_or("");
            write_field(&mut output, value);
        }
        output.push_str("\r\n");
    }
    debug_assert_eq!(output.len(), output_bytes);
    Ok(output)
}

fn parse_field(text: &str, index: &mut usize) -> ApiResult<(String, Terminator)> {
    let bytes = text.as_bytes();
    if *index == bytes.len() {
        return Ok((String::new(), Terminator::End));
    }
    if bytes[*index] == b'"' {
        parse_quoted_field(text, index)
    } else {
        let start = *index;
        while *index < bytes.len() {
            match bytes[*index] {
                b'\t' | b'\n' => break,
                b'\r' => {
                    if bytes.get(*index + 1) != Some(&b'\n') {
                        return Err(clipboard_error(
                            "clipboard TSV contains a carriage return outside CRLF",
                        ));
                    }
                    break;
                }
                _ => *index += 1,
            }
        }
        let field = text[start..*index].to_owned();
        let terminator = consume_terminator(bytes, index)?;
        Ok((field, terminator))
    }
}

fn parse_quoted_field(text: &str, index: &mut usize) -> ApiResult<(String, Terminator)> {
    let bytes = text.as_bytes();
    *index += 1; // opening quote
    let mut segment_start = *index;
    let mut value = String::new();

    while *index < bytes.len() {
        if bytes[*index] != b'"' {
            *index += 1;
            continue;
        }

        value.push_str(&text[segment_start..*index]);
        if bytes.get(*index + 1) == Some(&b'"') {
            value.push('"');
            *index += 2;
            segment_start = *index;
            continue;
        }

        *index += 1; // closing quote
        let terminator = consume_terminator(bytes, index)?;
        return Ok((value, terminator));
    }

    Err(clipboard_error(
        "clipboard TSV has an unterminated quoted field",
    ))
}

fn consume_terminator(bytes: &[u8], index: &mut usize) -> ApiResult<Terminator> {
    if *index == bytes.len() {
        return Ok(Terminator::End);
    }
    match bytes[*index] {
        b'\t' => {
            *index += 1;
            Ok(Terminator::Field)
        }
        b'\n' => {
            *index += 1;
            Ok(Terminator::Record)
        }
        b'\r' if bytes.get(*index + 1) == Some(&b'\n') => {
            *index += 2;
            Ok(Terminator::Record)
        }
        b'\r' => Err(clipboard_error(
            "clipboard TSV contains a carriage return outside CRLF",
        )),
        _ => Err(clipboard_error(
            "clipboard TSV has text after a closing quote",
        )),
    }
}

fn check_next_field(
    completed_rows: usize,
    current_columns: usize,
    max_columns: usize,
) -> ApiResult<()> {
    let next_columns = current_columns
        .checked_add(1)
        .ok_or_else(|| clipboard_error("clipboard TSV dimensions overflow"))?;
    if next_columns > MAX_CLIPBOARD_COLUMNS {
        return Err(clipboard_error(format!(
            "clipboard TSV exceeds the {MAX_CLIPBOARD_COLUMNS}-column limit"
        )));
    }

    let prospective_rows = completed_rows
        .checked_add(1)
        .ok_or_else(|| clipboard_error("clipboard TSV dimensions overflow"))?;
    let prospective_columns = max_columns.max(next_columns);
    let cells = prospective_rows
        .checked_mul(prospective_columns)
        .ok_or_else(|| clipboard_error("clipboard TSV dimensions overflow"))?;
    if cells > MAX_CLIPBOARD_CELLS {
        return Err(clipboard_error(format!(
            "clipboard TSV exceeds the {MAX_CLIPBOARD_CELLS}-cell limit"
        )));
    }
    Ok(())
}

fn finish_row(
    rows: &mut Vec<Vec<String>>,
    row: &mut Vec<String>,
    max_columns: &mut usize,
) -> ApiResult<()> {
    let next_rows = rows
        .len()
        .checked_add(1)
        .ok_or_else(|| clipboard_error("clipboard TSV dimensions overflow"))?;
    let next_columns = (*max_columns).max(row.len());
    if next_rows > MAX_CLIPBOARD_ROWS || next_columns > MAX_CLIPBOARD_COLUMNS {
        return Err(clipboard_error(
            "clipboard TSV dimensions exceed their limits",
        ));
    }
    let cells = next_rows
        .checked_mul(next_columns)
        .ok_or_else(|| clipboard_error("clipboard TSV dimensions overflow"))?;
    if cells > MAX_CLIPBOARD_CELLS {
        return Err(clipboard_error(format!(
            "clipboard TSV exceeds the {MAX_CLIPBOARD_CELLS}-cell limit"
        )));
    }

    *max_columns = next_columns;
    rows.push(std::mem::take(row));
    Ok(())
}

fn serialized_field_len(value: &str) -> ApiResult<usize> {
    let requires_quotes = needs_quotes(value);
    if !requires_quotes {
        return Ok(value.len());
    }
    let escaped_quotes = value
        .as_bytes()
        .iter()
        .filter(|byte| **byte == b'"')
        .count();
    value
        .len()
        .checked_add(escaped_quotes)
        .and_then(|length| length.checked_add(2))
        .ok_or_else(|| clipboard_error("clipboard TSV output size overflow"))
}

fn add_output_bytes(total: &mut usize, additional: usize) -> ApiResult<()> {
    *total = total
        .checked_add(additional)
        .ok_or_else(|| clipboard_error("clipboard TSV output size overflow"))?;
    if *total > MAX_CLIPBOARD_TSV_BYTES {
        return Err(clipboard_error(format!(
            "clipboard TSV exceeds the {MAX_CLIPBOARD_TSV_BYTES}-byte limit"
        )));
    }
    Ok(())
}

fn needs_quotes(value: &str) -> bool {
    value.is_empty()
        || value
            .as_bytes()
            .iter()
            .any(|byte| matches!(byte, b'"' | b'\t' | b'\r' | b'\n'))
}

fn write_field(output: &mut String, value: &str) {
    if !needs_quotes(value) {
        output.push_str(value);
        return;
    }

    output.push('"');
    for (part_index, part) in value.split('"').enumerate() {
        if part_index > 0 {
            output.push_str("\"\"");
        }
        output.push_str(part);
    }
    output.push('"');
}

fn clipboard_error(message: impl Into<String>) -> crate::ApiError {
    ApiUnsupportedError::Clipboard(message.into()).into()
}

#[cfg(test)]
mod tests {
    use super::{MAX_CLIPBOARD_CELLS, MAX_CLIPBOARD_TSV_BYTES, parse_tsv, serialize_tsv};

    #[test]
    fn parses_quoted_tabs_newlines_quotes_and_unicode() {
        let text =
            "\"left\tright\"\t\"русский\n😀 \"\"quote\"\"\"\t\"crlf\r\ninside\"\r\nобычный\tтекст";
        assert_eq!(
            parse_tsv(text).unwrap(),
            vec![
                vec![
                    "left\tright".to_owned(),
                    "русский\n😀 \"quote\"".to_owned(),
                    "crlf\r\ninside".to_owned(),
                ],
                vec!["обычный".to_owned(), "текст".to_owned(), String::new()],
            ]
        );
    }

    #[test]
    fn pads_irregular_rows_and_keeps_trailing_tabs_and_blank_rows() {
        assert_eq!(
            parse_tsv("a\tb\r\nc\n").unwrap(),
            vec![
                vec!["a".to_owned(), "b".to_owned()],
                vec!["c".to_owned(), String::new()]
            ]
        );
        assert_eq!(
            parse_tsv("x\t").unwrap(),
            vec![vec!["x".to_owned(), String::new()]]
        );
        assert_eq!(
            parse_tsv("a\n\n").unwrap(),
            vec![vec!["a".to_owned()], vec![String::new()]]
        );
        assert_eq!(parse_tsv("\n").unwrap(), vec![vec![String::new()]]);
        assert_eq!(parse_tsv("").unwrap(), vec![vec![String::new()]]);
        assert_eq!(
            parse_tsv("00123\t+123\n1-2").unwrap(),
            vec![
                vec!["00123".to_owned(), "+123".to_owned()],
                vec!["1-2".to_owned(), String::new()],
            ]
        );
    }

    #[test]
    fn serializer_quotes_fields_and_round_trips_with_terminal_crlf() {
        let rows = vec![
            vec!["tab\tvalue".to_owned(), "say \"hi\"".to_owned()],
            vec!["line\n😀".to_owned(), String::new()],
        ];
        let serialized = serialize_tsv(&rows).unwrap();
        assert!(serialized.ends_with("\r\n"));
        assert_eq!(parse_tsv(&serialized).unwrap(), rows);
        assert_eq!(serialize_tsv(&[vec![String::new()]]).unwrap(), "\"\"\r\n");

        let ragged = vec![
            vec!["one".to_owned()],
            vec!["two".to_owned(), "three".to_owned()],
        ];
        assert_eq!(
            parse_tsv(&serialize_tsv(&ragged).unwrap()).unwrap(),
            vec![
                vec!["one".to_owned(), String::new()],
                vec!["two".to_owned(), "three".to_owned()],
            ]
        );
    }

    #[test]
    fn accepts_quotes_inside_unquoted_fields_but_rejects_malformed_quoting() {
        assert_eq!(
            parse_tsv("a\"b\tplain").unwrap(),
            vec![vec!["a\"b".to_owned(), "plain".to_owned()]]
        );
        for malformed in ["\"open", "\"closed\"tail", "a\rb", "\"a\"\rb"] {
            assert!(parse_tsv(malformed).is_err(), "accepted {malformed:?}");
        }
    }

    #[test]
    fn rejects_oversized_input_output_and_rectangular_expansion() {
        let oversized = "x".repeat(MAX_CLIPBOARD_TSV_BYTES + 1);
        assert!(parse_tsv(&oversized).is_err());

        let oversized_output = vec![vec!["x".repeat(MAX_CLIPBOARD_TSV_BYTES)]];
        assert!(serialize_tsv(&oversized_output).is_err());

        let row = vec!["x"; 101].join("\t");
        let rectangle = std::iter::repeat_n(row.as_str(), 100)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rectangle.len() < MAX_CLIPBOARD_TSV_BYTES);
        assert!(parse_tsv(&rectangle).is_err());

        let too_many_cells = vec![vec![String::new(); MAX_CLIPBOARD_CELLS + 1]];
        assert!(serialize_tsv(&too_many_cells).is_err());
    }
}
