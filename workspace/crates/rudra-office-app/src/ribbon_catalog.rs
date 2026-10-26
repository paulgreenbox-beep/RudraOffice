//! Commands exposed by the app's additional ribbon tabs.
//! Home keeps its existing, module-specific widgets and dispatch.

use crate::Module;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum RibbonTab {
    #[default]
    Home,
    Insert,
    Layout,
    Formulas,
    Data,
    View,
}

impl RibbonTab {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Home => "Главная",
            Self::Insert => "Вставка",
            Self::Layout => "Макет",
            Self::Formulas => "Формулы",
            Self::Data => "Данные",
            Self::View => "Вид",
        }
    }

    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Insert => "insert",
            Self::Layout => "layout",
            Self::Formulas => "formulas",
            Self::Data => "data",
            Self::View => "view",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RibbonCommand {
    InsertParagraph,
    InsertTable,
    AddSheet,
    DuplicateSheet,
    InsertRows,
    DeleteRows,
    InsertColumns,
    DeleteColumns,
    SortAscending,
    SortDescending,
    ApplyAutoFilter,
    ClearAutoFilter,
    GoToRange,
    FindNext,
    ClearRange,
    MergeSelection,
    UnmergeSelection,
    AddSlide,
    DuplicateSlide,
    Portrait,
    Landscape,
    PageA4,
    NormalMargins,
    NarrowMargins,
    RowHeight32,
    RowHeight48,
    ColumnWidth120,
    ColumnWidth180,
    ToggleWrap,
    AutoSum,
    GoToA1,
    ClearCell,
    PreviousSheet,
    NextSheet,
    ZoomOut,
    ZoomIn,
    ZoomReset,
    PrintLayout,
    PlainText,
    FitWidth,
    FitPage,
    PresentSlides,
}

impl RibbonCommand {
    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::InsertParagraph => "insert-paragraph",
            Self::InsertTable => "insert-table",
            Self::AddSheet => "add-sheet",
            Self::DuplicateSheet => "duplicate-sheet",
            Self::InsertRows => "insert-rows",
            Self::DeleteRows => "delete-rows",
            Self::InsertColumns => "insert-columns",
            Self::DeleteColumns => "delete-columns",
            Self::SortAscending => "sort-ascending",
            Self::SortDescending => "sort-descending",
            Self::ApplyAutoFilter => "apply-auto-filter",
            Self::ClearAutoFilter => "clear-auto-filter",
            Self::GoToRange => "go-to-range",
            Self::FindNext => "find-next",
            Self::ClearRange => "clear-range",
            Self::MergeSelection => "merge-selection",
            Self::UnmergeSelection => "unmerge-selection",
            Self::AddSlide => "add-slide",
            Self::DuplicateSlide => "duplicate-slide",
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
            Self::PageA4 => "page-a4",
            Self::NormalMargins => "normal-margins",
            Self::NarrowMargins => "narrow-margins",
            Self::RowHeight32 => "row-height-32",
            Self::RowHeight48 => "row-height-48",
            Self::ColumnWidth120 => "column-width-120",
            Self::ColumnWidth180 => "column-width-180",
            Self::ToggleWrap => "toggle-wrap",
            Self::AutoSum => "auto-sum",
            Self::GoToA1 => "go-to-a1",
            Self::ClearCell => "clear-cell",
            Self::PreviousSheet => "previous-sheet",
            Self::NextSheet => "next-sheet",
            Self::ZoomOut => "zoom-out",
            Self::ZoomIn => "zoom-in",
            Self::ZoomReset => "zoom-reset",
            Self::PrintLayout => "print-layout",
            Self::PlainText => "plain-text",
            Self::FitWidth => "fit-width",
            Self::FitPage => "fit-page",
            Self::PresentSlides => "present-slides",
        }
    }
}

pub(super) fn tabs_for_format(format: Module) -> &'static [RibbonTab] {
    use RibbonTab::*;
    match format {
        Module::Home => &[],
        Module::Writer => &[Home, Insert, Layout, View],
        Module::Sheets => &[Home, Insert, Layout, Formulas, Data, View],
        Module::Slides => &[Home, Insert, View],
        Module::Pdf => &[Home, View],
    }
}

pub(super) fn commands_for(
    format: Module,
    tab: RibbonTab,
) -> &'static [(RibbonCommand, &'static str)] {
    use RibbonCommand::*;
    match (format, tab) {
        (Module::Writer, RibbonTab::Insert) => &[
            (InsertParagraph, "Новый абзац"),
            (InsertTable, "Таблица 2 × 2"),
        ],
        (Module::Writer, RibbonTab::Layout) => &[
            (Portrait, "Книжная"),
            (Landscape, "Альбомная"),
            (PageA4, "Размер A4"),
            (NormalMargins, "Обычные поля"),
            (NarrowMargins, "Узкие поля"),
        ],
        (Module::Sheets, RibbonTab::Insert) => &[
            (AddSheet, "Добавить лист"),
            (DuplicateSheet, "Дублировать лист"),
            (InsertRows, "Вставить строки"),
            (DeleteRows, "Удалить строки"),
            (InsertColumns, "Вставить столбцы"),
            (DeleteColumns, "Удалить столбцы"),
            (MergeSelection, "Объединить ячейки"),
            (UnmergeSelection, "Разъединить ячейки"),
        ],
        (Module::Sheets, RibbonTab::Layout) => &[
            (RowHeight32, "Высота строки: 32"),
            (RowHeight48, "Высота строки: 48"),
            (ColumnWidth120, "Ширина столбца: 120"),
            (ColumnWidth180, "Ширина столбца: 180"),
            (ToggleWrap, "Перенос текста"),
        ],
        (Module::Sheets, RibbonTab::Formulas) => &[(AutoSum, "Автосумма")],
        (Module::Sheets, RibbonTab::Data) => &[
            (SortAscending, "По возрастанию"),
            (SortDescending, "По убыванию"),
            (ApplyAutoFilter, "Фильтр"),
            (ClearAutoFilter, "Очистить фильтр"),
            (GoToRange, "Перейти к адресу"),
            (FindNext, "Найти далее"),
            (ClearRange, "Очистить выделение"),
            (GoToA1, "Перейти к A1"),
            (PreviousSheet, "Предыдущий лист"),
            (NextSheet, "Следующий лист"),
        ],
        (Module::Slides, RibbonTab::Insert) => &[
            (AddSlide, "Добавить слайд"),
            (DuplicateSlide, "Дублировать слайд"),
        ],
        (Module::Writer, RibbonTab::View) => &[
            (ZoomOut, "Уменьшить"),
            (ZoomIn, "Увеличить"),
            (ZoomReset, "Масштаб 100%"),
            (PrintLayout, "Разметка страницы"),
            (PlainText, "Текстовый вид"),
        ],
        (Module::Sheets, RibbonTab::View) => &[
            (ZoomOut, "Уменьшить"),
            (ZoomIn, "Увеличить"),
            (ZoomReset, "Масштаб 100%"),
        ],
        (Module::Slides, RibbonTab::View) => &[
            (ZoomOut, "Уменьшить"),
            (ZoomIn, "Увеличить"),
            (ZoomReset, "Масштаб 100%"),
            (PresentSlides, "Показ слайдов"),
        ],
        (Module::Pdf, RibbonTab::View) => &[
            (ZoomOut, "Уменьшить"),
            (ZoomIn, "Увеличить"),
            (ZoomReset, "Масштаб 100%"),
            (FitWidth, "По ширине"),
            (FitPage, "Страница целиком"),
        ],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULES: [Module; 5] = [
        Module::Home,
        Module::Writer,
        Module::Sheets,
        Module::Slides,
        Module::Pdf,
    ];
    const TABS: [RibbonTab; 6] = [
        RibbonTab::Home,
        RibbonTab::Insert,
        RibbonTab::Layout,
        RibbonTab::Formulas,
        RibbonTab::Data,
        RibbonTab::View,
    ];

    #[test]
    fn tabs_follow_editor_capabilities() {
        use RibbonTab::*;
        assert_eq!(RibbonTab::default(), Home);
        assert!(tabs_for_format(Module::Home).is_empty());
        assert_eq!(
            tabs_for_format(Module::Writer),
            &[Home, Insert, Layout, View]
        );
        assert_eq!(
            tabs_for_format(Module::Sheets),
            &[Home, Insert, Layout, Formulas, Data, View]
        );
        assert_eq!(tabs_for_format(Module::Slides), &[Home, Insert, View]);
        assert_eq!(tabs_for_format(Module::Pdf), &[Home, View]);
    }

    #[test]
    fn sheets_insert_and_data_commands_match_the_ui_contract() {
        use RibbonCommand::*;
        assert_eq!(
            commands_for(Module::Sheets, RibbonTab::Insert),
            &[
                (AddSheet, "Добавить лист"),
                (DuplicateSheet, "Дублировать лист"),
                (InsertRows, "Вставить строки"),
                (DeleteRows, "Удалить строки"),
                (InsertColumns, "Вставить столбцы"),
                (DeleteColumns, "Удалить столбцы"),
                (MergeSelection, "Объединить ячейки"),
                (UnmergeSelection, "Разъединить ячейки"),
            ]
        );
        assert_eq!(InsertRows.id(), "insert-rows");
        assert_eq!(DeleteRows.id(), "delete-rows");
        assert_eq!(InsertColumns.id(), "insert-columns");
        assert_eq!(DeleteColumns.id(), "delete-columns");
        assert_eq!(
            commands_for(Module::Sheets, RibbonTab::Data),
            &[
                (SortAscending, "По возрастанию"),
                (SortDescending, "По убыванию"),
                (ApplyAutoFilter, "Фильтр"),
                (ClearAutoFilter, "Очистить фильтр"),
                (GoToRange, "Перейти к адресу"),
                (FindNext, "Найти далее"),
                (ClearRange, "Очистить выделение"),
                (GoToA1, "Перейти к A1"),
                (PreviousSheet, "Предыдущий лист"),
                (NextSheet, "Следующий лист"),
            ]
        );
        assert_eq!(SortAscending.id(), "sort-ascending");
        assert_eq!(SortDescending.id(), "sort-descending");
        assert_eq!(ApplyAutoFilter.id(), "apply-auto-filter");
        assert_eq!(ClearAutoFilter.id(), "clear-auto-filter");
        assert_eq!(ClearCell.id(), "clear-cell");
        assert!(
            commands_for(Module::Sheets, RibbonTab::Data)
                .iter()
                .all(|&(command, _)| command != ClearCell)
        );
    }

    #[test]
    fn unsupported_tabs_have_no_commands() {
        for module in MODULES {
            for tab in TABS {
                let commands = commands_for(module, tab);
                if !tabs_for_format(module).contains(&tab) || tab == RibbonTab::Home {
                    assert!(commands.is_empty(), "{module:?}/{tab:?}");
                } else {
                    assert!(!commands.is_empty(), "{module:?}/{tab:?}");
                }
            }
        }
    }

    #[test]
    fn commands_do_not_cross_editor_boundaries() {
        use RibbonCommand::*;
        for module in MODULES {
            for tab in TABS {
                for &(command, _) in commands_for(module, tab) {
                    let valid = match command {
                        ZoomOut | ZoomIn | ZoomReset => {
                            module != Module::Home && tab == RibbonTab::View
                        }
                        InsertParagraph | InsertTable => {
                            module == Module::Writer && tab == RibbonTab::Insert
                        }
                        Portrait | Landscape | PageA4 | NormalMargins | NarrowMargins => {
                            module == Module::Writer && tab == RibbonTab::Layout
                        }
                        PrintLayout | PlainText => {
                            module == Module::Writer && tab == RibbonTab::View
                        }
                        AddSheet | DuplicateSheet | InsertRows | DeleteRows | InsertColumns
                        | DeleteColumns | MergeSelection | UnmergeSelection => {
                            module == Module::Sheets && tab == RibbonTab::Insert
                        }
                        RowHeight32 | RowHeight48 | ColumnWidth120 | ColumnWidth180
                        | ToggleWrap => module == Module::Sheets && tab == RibbonTab::Layout,
                        AutoSum => module == Module::Sheets && tab == RibbonTab::Formulas,
                        SortAscending | SortDescending | ApplyAutoFilter | ClearAutoFilter
                        | GoToRange | FindNext | ClearRange | GoToA1 | ClearCell
                        | PreviousSheet | NextSheet => {
                            module == Module::Sheets && tab == RibbonTab::Data
                        }
                        AddSlide | DuplicateSlide => {
                            module == Module::Slides && tab == RibbonTab::Insert
                        }
                        PresentSlides => module == Module::Slides && tab == RibbonTab::View,
                        FitWidth | FitPage => module == Module::Pdf && tab == RibbonTab::View,
                    };
                    assert!(valid, "{module:?}/{tab:?}/{command:?}");
                }
            }
        }
    }

    #[test]
    fn buttons_fit_the_ribbon_and_have_unique_stable_ids() {
        let mut known_commands = Vec::new();
        for (index, tab) in TABS.iter().enumerate() {
            assert!(!tab.label().trim().is_empty());
            assert!(valid_id(tab.id()));
            assert!(TABS[..index].iter().all(|other| other.id() != tab.id()));
        }
        for module in MODULES {
            let tabs = tabs_for_format(module);
            for (index, tab) in tabs.iter().enumerate() {
                assert!(!tabs[..index].contains(tab));
                let commands = commands_for(module, *tab);
                assert!(commands.len() <= 10, "{module:?}/{tab:?}");
                for (index, &(command, label)) in commands.iter().enumerate() {
                    assert!(!label.trim().is_empty());
                    assert!(valid_id(command.id()));
                    assert!(commands[..index].iter().all(|&(other, _)| other != command));
                    if !known_commands.contains(&command) {
                        assert!(
                            known_commands
                                .iter()
                                .all(|other: &RibbonCommand| other.id() != command.id())
                        );
                        known_commands.push(command);
                    }
                }
            }
        }
    }

    fn valid_id(id: &str) -> bool {
        !id.is_empty()
            && !id.starts_with('-')
            && !id.ends_with('-')
            && !id.contains("--")
            && id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    }
}
