//! Sheet-scoped navigation/search state; document mutations stay in Sheets API.
use sheets_core::{CellAddress, ColumnIndex, FilterCriterion};
use sheets_ui::UiCell;

pub(super) const FILTER_LABELS: [&str; 9] = [
    "Равно",
    "Не равно",
    "Содержит",
    "Пустые",
    "Непустые",
    "Число равно",
    "Больше",
    "Меньше",
    "Значения (по строкам)",
];

const MAX_FILTER_INPUT_BYTES: usize = 4096;
const MAX_FILTER_VALUES: usize = 128;
const MAX_FILTER_TEXT_CHARS: usize = 256;

pub(super) fn mutation_error(error: sheets_api::ApiError) -> String {
    use sheets_api::{ApiErrorKind, ApiUnsupportedError};
    match error.kind() {
        ApiErrorKind::Transaction(sheets_api::ApiTransactionError::HistoryBudgetExceeded {
            required_cell_endpoints,
            required_bytes,
            limits,
        }) => {
            if limits.max_entries == 0 {
                "Операция отклонена: история отмены отключена. Данные и история не изменены.".into()
            } else {
                format!(
                    "Операция отклонена: превышен бюджет отмены (нужно {} ячеек/{} байт; лимит {} ячеек/{} байт). Уменьшите диапазон. Данные и история не изменены.",
                    required_cell_endpoints, required_bytes,
                    limits.max_retained_cells, limits.max_retained_bytes
                )
            }
        }
        ApiErrorKind::Unsupported(ApiUnsupportedError::MergeWouldConcealInput { address }) => {
            format!(
                "Объединение скроет содержимое {}{}. Сначала переместите или очистите эту ячейку.",
                crate::column_label(address.column),
                address.row.0 + 1
            )
        }
        ApiErrorKind::Unsupported(ApiUnsupportedError::MergeOverlapsExisting { .. }) => {
            "Диапазон пересекает существующее объединение. Сначала разъедините ячейки.".into()
        }
        ApiErrorKind::Unsupported(ApiUnsupportedError::SortMergedRange { .. }) => {
            "Сортировка отклонена: диапазон пересекает объединённые ячейки. Разъедините их перед сортировкой; данные и история не изменены.".into()
        }
        ApiErrorKind::Unsupported(ApiUnsupportedError::SortFormula(reason)) => {
            format!("Сортировка отклонена: IronCalc не может безопасно перенести формулу. {}",
                reason.chars().take(220).collect::<String>())
        }
        ApiErrorKind::Unsupported(ApiUnsupportedError::StructuralEdit(reason)) => {
            format!("Невозможно безопасно изменить строки или столбцы: {}. Данные и история не изменены.",
                reason.chars().take(240).collect::<String>())
        }
        _ => error.to_string(),
    }
}

#[derive(Clone, Debug)]
pub(super) struct SheetsToolsDraft {
    pub address: String,
    pub query: String,
    pub last_match: Option<CellAddress>,
    pub key_column: String,
    pub has_header: bool,
    pub filter_kind: usize,
    pub filter_value: String,
}

impl Default for SheetsToolsDraft {
    fn default() -> Self {
        Self {
            address: "A1".into(),
            query: String::new(),
            last_match: None,
            key_column: "A".into(),
            has_header: false,
            filter_kind: 0,
            filter_value: String::new(),
        }
    }
}

/// Parse an Excel column label and require it to lie within the selected range.
pub(super) fn parse_key_column(
    input: &str,
    start: ColumnIndex,
    end: ColumnIndex,
) -> Result<ColumnIndex, String> {
    if input.is_empty() || input.len() > 3 || !input.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return Err("Укажите столбец латинскими буквами от A до XFD.".into());
    }

    let mut one_based = 0u32;
    for byte in input.bytes() {
        one_based = one_based * 26 + u32::from(byte.to_ascii_uppercase() - b'A' + 1);
    }
    if one_based > 16_384 {
        return Err("Номер столбца не может быть больше XFD.".into());
    }

    let column = ColumnIndex(one_based - 1);
    let lower = start.0.min(end.0);
    let upper = start.0.max(end.0);
    if !(lower..=upper).contains(&column.0) {
        return Err("Столбец должен находиться в выбранном диапазоне.".into());
    }
    Ok(column)
}

/// Parse a bounded, UI-owned filter draft without touching sheet state.
pub(super) fn parse_filter_criterion(kind: usize, value: &str) -> Result<FilterCriterion, String> {
    if value.len() > MAX_FILTER_INPUT_BYTES {
        return Err("Значение фильтра превышает 4096 байт.".into());
    }

    match kind {
        0 => Ok(FilterCriterion::Equals(parse_filter_text(value)?)),
        1 => Ok(FilterCriterion::NotEquals(parse_filter_text(value)?)),
        2 => Ok(FilterCriterion::Contains(parse_filter_text(value)?)),
        3 => Ok(FilterCriterion::Blank),
        4 => Ok(FilterCriterion::NonBlank),
        5 => Ok(FilterCriterion::NumberEquals(parse_filter_number(value)?)),
        6 => Ok(FilterCriterion::GreaterThan(parse_filter_number(value)?)),
        7 => Ok(FilterCriterion::LessThan(parse_filter_number(value)?)),
        8 => Ok(FilterCriterion::Values(parse_filter_values(value)?)),
        _ => Err("Неизвестный тип фильтра.".into()),
    }
}

fn parse_filter_text(value: &str) -> Result<String, String> {
    if value.chars().count() > MAX_FILTER_TEXT_CHARS {
        return Err("Текст фильтра превышает 256 символов.".into());
    }
    Ok(value.to_owned())
}

fn parse_filter_number(value: &str) -> Result<f64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Укажите число для фильтра.".into());
    }

    let normalized = if trimmed.contains(',') {
        if trimmed.contains('.') || trimmed.matches(',').count() != 1 {
            return Err("Укажите число с одной десятичной точкой или запятой.".into());
        }
        Some(trimmed.replace(',', "."))
    } else {
        None
    };
    let number = normalized
        .as_deref()
        .unwrap_or(trimmed)
        .parse::<f64>()
        .map_err(|_| "Не удалось разобрать число фильтра.".to_string())?;
    if !number.is_finite() {
        return Err("Число фильтра должно быть конечным.".into());
    }
    Ok(number)
}

fn parse_filter_values(value: &str) -> Result<Vec<String>, String> {
    let mut values = Vec::new();
    for line in value.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.chars().count() > MAX_FILTER_TEXT_CHARS {
            return Err("Каждое значение фильтра ограничено 256 символами.".into());
        }
        if values
            .iter()
            .any(|existing: &String| existing.as_str() == line)
        {
            continue;
        }
        if values.len() == MAX_FILTER_VALUES {
            return Err("Фильтр поддерживает не более 128 значений.".into());
        }
        values.push(line.to_owned());
    }
    Ok(values)
}

/// Case-insensitive row-major search over materialized cells only, with wrap.
/// Search display values AND raw formula/input, never allocate a full grid.
pub(super) fn next_match(
    cells: &[UiCell],
    query: &str,
    after: Option<CellAddress>,
) -> Option<CellAddress> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let mut matches = cells
        .iter()
        .filter(|cell| {
            cell.display.to_lowercase().contains(&query) || cell.raw.to_lowercase().contains(&query)
        })
        .map(|cell| cell.address)
        .collect::<Vec<_>>();
    matches.sort_by_key(|address| (address.row.0, address.column.0));
    matches
        .iter()
        .copied()
        .find(|address| {
            after.is_none_or(|after| {
                address.sheet != after.sheet
                    || (address.row.0, address.column.0) > (after.row.0, after.column.0)
            })
        })
        .or_else(|| matches.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sheets_core::{RowIndex, SheetId};
    fn cell(row: u32, column: u32, display: &str, raw: &str) -> UiCell {
        UiCell {
            address: CellAddress::new(SheetId(1), RowIndex(row), ColumnIndex(column)),
            display: display.into(),
            raw: raw.into(),
            format: None,
        }
    }
    #[test]
    fn search_uses_unicode_case_row_order_and_wrap() {
        let cells = [
            cell(2, 0, "ЛОТОК", "ЛОТОК"),
            cell(0, 1, "лоток", "лоток"),
            cell(0, 0, "крепление", "крепление"),
        ];
        assert_eq!(next_match(&cells, "ЛоТоК", None), Some(cells[1].address));
        assert_eq!(
            next_match(&cells, "лоток", Some(cells[1].address)),
            Some(cells[0].address)
        );
        assert_eq!(
            next_match(&cells, "лоток", Some(cells[0].address)),
            Some(cells[1].address)
        );
    }
    #[test]
    fn search_includes_raw_formulas_and_rejects_empty_query() {
        let cells = [cell(0, 0, "30", "=SUM(A1:A2)")];
        assert_eq!(next_match(&cells, "sum", None), Some(cells[0].address));
        assert_eq!(next_match(&cells, "  ", None), None);
        assert_eq!(next_match(&cells, "absent", None), None);
    }

    #[test]
    fn draft_parser_defaults_seed_key_and_filter_fields() {
        let draft = SheetsToolsDraft::default();
        assert_eq!(draft.key_column, "A");
        assert!(!draft.has_header);
        assert_eq!(draft.filter_kind, 0);
        assert_eq!(draft.filter_value, "");
    }

    #[test]
    fn draft_parser_key_column_is_case_insensitive_and_range_bounded() {
        assert_eq!(
            parse_key_column("xfd", ColumnIndex(0), ColumnIndex(16_383)),
            Ok(ColumnIndex(16_383))
        );
        assert_eq!(
            parse_key_column("C", ColumnIndex(3), ColumnIndex(1)),
            Ok(ColumnIndex(2))
        );
        assert!(parse_key_column("C", ColumnIndex(0), ColumnIndex(1)).is_err());
    }

    #[test]
    fn draft_parser_rejects_invalid_out_of_range_and_non_ascii_columns() {
        for invalid in ["", "A1", "AAAA", "XFE", " A"] {
            assert!(parse_key_column(invalid, ColumnIndex(0), ColumnIndex(16_383)).is_err());
        }
        assert!(parse_key_column("К", ColumnIndex(0), ColumnIndex(16_383)).is_err());
        assert!(parse_key_column("XFD", ColumnIndex(0), ColumnIndex(16_382)).is_err());
    }

    #[test]
    fn draft_parser_maps_cyrillic_text_values_and_numeric_filters() {
        assert_eq!(
            parse_filter_criterion(0, "точно").unwrap(),
            FilterCriterion::Equals("точно".into())
        );
        assert_eq!(
            parse_filter_criterion(1, "иначе").unwrap(),
            FilterCriterion::NotEquals("иначе".into())
        );
        assert_eq!(
            parse_filter_criterion(2, "Крепление").unwrap(),
            FilterCriterion::Contains("Крепление".into())
        );
        assert_eq!(
            parse_filter_criterion(8, "Кабель, NYM\r\n\r\nЛоток\nКабель, NYM").unwrap(),
            FilterCriterion::Values(vec!["Кабель, NYM".into(), "".into(), "Лоток".into()])
        );
        assert_eq!(
            parse_filter_criterion(5, "  -12,5  ").unwrap(),
            FilterCriterion::NumberEquals(-12.5)
        );
        assert_eq!(
            parse_filter_criterion(6, "1.25").unwrap(),
            FilterCriterion::GreaterThan(1.25)
        );
        assert_eq!(
            parse_filter_criterion(7, "-2").unwrap(),
            FilterCriterion::LessThan(-2.0)
        );
        assert_eq!(
            parse_filter_criterion(3, "").unwrap(),
            FilterCriterion::Blank
        );
        assert_eq!(
            parse_filter_criterion(4, "").unwrap(),
            FilterCriterion::NonBlank
        );
    }

    #[test]
    fn draft_parser_enforces_filter_text_value_and_input_bounds() {
        assert!(parse_filter_criterion(0, &"Я".repeat(257)).is_err());
        assert!(parse_filter_criterion(3, &"x".repeat(4097)).is_err());
        assert!(parse_filter_criterion(8, &"я".repeat(257)).is_err());

        let accepted = (0..MAX_FILTER_VALUES)
            .map(|value| format!("v{value}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            match parse_filter_criterion(8, &accepted).unwrap() {
                FilterCriterion::Values(values) => values.len(),
                other => panic!("unexpected criterion: {other:?}"),
            },
            MAX_FILTER_VALUES
        );
        let too_many = (0..=MAX_FILTER_VALUES)
            .map(|value| format!("v{value}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(parse_filter_criterion(8, &too_many).is_err());
        let duplicates = vec!["Кабель"; MAX_FILTER_VALUES + 1].join("\n");
        assert_eq!(
            parse_filter_criterion(8, &duplicates).unwrap(),
            FilterCriterion::Values(vec!["Кабель".into()])
        );
    }

    #[test]
    fn draft_parser_rejects_non_finite_malformed_numbers_and_unknown_filters() {
        for invalid in ["", "NaN", "inf", "1e309", "1,2,3", "1,2.3"] {
            assert!(parse_filter_criterion(5, invalid).is_err());
        }
        assert!(parse_filter_criterion(9, "value").is_err());
    }
}
