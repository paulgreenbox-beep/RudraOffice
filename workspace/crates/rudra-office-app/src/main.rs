pub use makepad_widgets;

mod app_accessibility;
mod app_documents;
mod app_events;
mod app_launcher;
mod app_pdf;
mod app_sheets;
mod app_shell;
mod app_slides;
mod app_ui;
mod app_writer;
mod app_writer_view;

#[cfg(windows)]
mod accessibility_bridge;
mod blank_documents;
mod document_runtime;
mod document_tabs;
mod file_admission;
mod new_document_save;
mod ribbon_catalog;
mod sheets_clipboard_state;
#[cfg(test)]
mod sheets_clipboard_tests;
mod windows_clipboard;
#[cfg(windows)]
mod windows_file_picker;
#[cfg(windows)]
mod windows_grid_paste_hook;
#[cfg(windows)]
type NativeGridPasteOwner = windows_grid_paste_hook::NativeGridPasteHook;
#[cfg(not(windows))]
type NativeGridPasteOwner = ();
type NativeGridPasteSlot = std::cell::RefCell<Option<NativeGridPasteOwner>>;
mod writer_page_view;
mod writer_runtime_adapter;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[cfg(windows)]
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    mpsc::{self, Receiver, TryRecvError},
};
use std::thread;

use makepad_widgets::makepad_draw::text::selection::Cursor as MakepadTextCursor;
use makepad_widgets::makepad_platform::{CxOsOp, thread::SignalToUI};
use makepad_widgets::*;
use pdf_core::{DocumentId, DocumentInfo, PageGeometry, PageId, PdfEngine};
use pdf_oxide_adapter::{
    PdfOpenArtifact, PdfOxideAdapter, PdfRenderRequest, PdfRenderWorker, PdfViewerSafetyHint,
};
use sheets_api::{
    ClipboardPayload, EntryParser, FormatPatch as SheetsFormatPatch, FormatPatchBounds,
    Rgba8 as SheetsRgba8, SpreadsheetApi,
};
use sheets_clipboard_state::{
    ClipboardClaim, MAX_CLIPBOARD_TEXT_BYTES, SheetsClipboardCommand, SheetsClipboardState,
};
#[cfg(test)]
use sheets_core::CellSnapshot;
use sheets_core::{
    AutoFilter, CellAddress, CellInput, CellRange, ColumnIndex, RowIndex, SheetId, SheetSnapshot,
    WorkbookSnapshot,
};
use sheets_ironcalc_adapter::IroncalcAdapter;
use sheets_ui::{SheetsUi, UiConfig, UiEvent, UiSnapshot};
#[cfg(test)]
mod sheets_history_tests;
mod sheets_selection_view_state;
#[cfg(test)]
mod sheets_sort_filter_tests;
#[cfg(test)]
mod sheets_structural_tests;
mod sheets_structural_tools;
mod sheets_tools;
use sheets_tools::{SheetsToolsDraft, next_match as next_sheets_match};
use sheets_xlsx::{
    BOUNDED_CELLS_PER_SHEET, BOUNDED_WINDOW_COLUMNS, BOUNDED_WINDOW_ROWS, BoundedSheetWindow,
    IroncalcXlsxParts, XlsxPartsCodec, import_bounded_sheet_window, uses_bounded_import,
};
use slides_api::{Color as SlidesColor, ObjectId, ObjectKind, Presentation, SlideId, SlideLayout};
use slides_core::{EditorCommand as SlidesEditorCommand, PresentationEditor};
use slides_pptx_adapter::office_oxide_adapter;
use slides_ui::{PresentationViewer, ViewerAction as SlidesViewerAction};
use writer_api::WriterApi;
use writer_api::core::{
    Alignment as WriterAlignment, BlockId, BlockSnapshot, Command as WriterCommand, Cursor,
    ParagraphStylePatch, Rgba8, Selection, TextContainerId, TextOffset, TextRange,
};
use writer_ui::{ToolbarAction, ToolbarApplyStatus, ToolbarValue, ViewMode, WriterViewModel};

#[cfg(windows)]
use windows_uia::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, TRUE},
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsZoomed,
            SW_MAXIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, ShowWindow,
        },
    },
    core::BOOL,
};

#[cfg(windows)]
use toolkit_windows_automation::{
    AccessibilityAction, AccessibilityActionRequest, AccessibilityActions, AccessibilityNode,
    AccessibilityRole, AccessibilitySelection, AccessibilityTree,
};

app_main!(App);

mod sheets_grid_widget;
#[cfg(windows)]
use sheets_grid_widget::SheetsGridAccessibilitySnapshot;
use sheets_grid_widget::{SheetsGridAction, SheetsGridCanvas};
#[cfg(test)]
mod pdf_async_tests;
mod pdf_page_widget;
mod pdf_render_identity;
mod pdf_texture_identity;
use pdf_page_widget::{PdfPageCanvas, PdfPageCanvasAction};
mod slides_visual_widget;
#[cfg(windows)]
use accessibility_bridge::{AccessibilityUiAction, AccessibilityWindow};
use blank_documents::BlankDocumentFormat;
use document_runtime::{
    DocumentId as RuntimeDocumentId, DocumentOperation, DocumentRegistry, DocumentRuntimeError,
    DocumentSession, DocumentSource, DocumentViewMode, DocumentViewState, PdfSessionView,
};
use document_tabs::{
    DocumentTabFrame, DocumentTabStrip, DocumentTabsAction, active_after_close, cycle_id,
    project_document_tabs,
};
use file_admission::{admit, read_after_admission};
use office_ui_runtime::{ScreenRect, VirtualCanvas, VirtualCanvasAction};
use ribbon_catalog::{RibbonCommand, RibbonTab, commands_for, tabs_for_format};
#[cfg(windows)]
use slides_visual_widget::{SlidesAccessibilityKind, SlidesVisualAccessibilitySnapshot};
use slides_visual_widget::{SlidesViewerFrame, SlidesVisualAction, SlidesVisualCanvas};
#[cfg(test)]
use writer_page_view::build_writer_page_frame;
use writer_page_view::{
    PIXELS_PER_HALF_POINT_AT_100, WriterPageFrameViewState,
    build_writer_page_frame_for_active_block,
};
use writer_runtime_adapter::{
    WRITER_FOOTER_DISTANCE_MARKER_ID, WRITER_HEADER_DISTANCE_MARKER_ID,
    WRITER_PARAGRAPH_FIRST_LINE_MARKER_PREFIX, WRITER_PARAGRAPH_LEFT_MARKER_PREFIX,
    WRITER_PARAGRAPH_RIGHT_MARKER_PREFIX, writer_block_id_from_paragraph_interaction_id,
    writer_runtime_frame,
};

const PRODUCT_BUILD_ID: &str = env!("RUDRA_PRODUCT_BUILD_ID");
#[cfg(windows)]
const WINDOW_TITLE: &str = "Rudra Office";

#[cfg(windows)]
fn rudra_office_hwnd() -> Result<HWND, String> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let handles = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        handles.push(hwnd);
        TRUE
    }

    fn title(hwnd: HWND) -> String {
        unsafe {
            let length = GetWindowTextLengthW(hwnd).max(0) as usize;
            let mut buffer = vec![0u16; length.saturating_add(1).max(1)];
            let read = GetWindowTextW(hwnd, &mut buffer).max(0) as usize;
            String::from_utf16_lossy(&buffer[..read])
        }
    }

    let mut handles = Vec::<HWND>::new();
    unsafe {
        EnumWindows(
            Some(collect),
            LPARAM((&mut handles as *mut Vec<HWND>) as isize),
        )
    }
    .map_err(|error| error.to_string())?;
    handles
        .into_iter()
        .find(|hwnd| {
            let mut process_id = 0;
            unsafe { GetWindowThreadProcessId(*hwnd, Some(&mut process_id)) };
            process_id == std::process::id() && title(*hwnd) == WINDOW_TITLE
        })
        .ok_or_else(|| "current process Rudra Office window was not found".into())
}

#[cfg(windows)]
fn office_window_work_area(hwnd: HWND) -> Result<RECT, String> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.0.is_null() {
        return Err("the nearest monitor could not be resolved".into());
    }
    let mut monitor_info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut monitor_info).as_bool() } {
        return Err("the monitor work area could not be read".into());
    }
    Ok(monitor_info.rcWork)
}

#[cfg(windows)]
fn fit_office_window_to_work_area(hwnd: HWND, work_area: RECT) -> Result<(), String> {
    let width = work_area.right.saturating_sub(work_area.left);
    let height = work_area.bottom.saturating_sub(work_area.top);
    if width <= 0 || height <= 0 {
        return Err("the monitor work area has invalid dimensions".into());
    }
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            work_area.left,
            work_area.top,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .map_err(|error| error.to_string())
}

fn minimize_office_window(cx: &mut Cx, ui: &WidgetRef) {
    let window = ui.window(cx, ids!(main_window));
    if let Some(window_id) = window.window_id() {
        cx.push_unique_platform_op(CxOsOp::MinimizeWindow(window_id));
    }
}

fn toggle_office_window_maximized(cx: &mut Cx, ui: &WidgetRef) {
    #[cfg(windows)]
    {
        match rudra_office_hwnd() {
            Ok(hwnd) => {
                if unsafe { IsZoomed(hwnd).as_bool() } {
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                } else {
                    unsafe {
                        let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                    }
                    if let Err(error) = office_window_work_area(hwnd)
                        .and_then(|work_area| fit_office_window_to_work_area(hwnd, work_area))
                    {
                        eprintln!(
                            "Rudra Office could not fit its maximized window to the work area: {error}"
                        );
                    }
                }
                return;
            }
            Err(error) => eprintln!("Rudra Office could not resolve its window: {error}"),
        }
    }

    let window = ui.window(cx, ids!(main_window));
    let is_maximized = window.is_fullscreen(cx);
    if let Some(window_id) = window.window_id() {
        cx.push_unique_platform_op(if is_maximized {
            CxOsOp::RestoreWindow(window_id)
        } else {
            CxOsOp::MaximizeWindow(window_id)
        });
    }
}

fn chrome_drag_target(header: Rect, controls: &[Rect], tab_bounds: &[Rect], point: DVec2) -> bool {
    let contains = |rect: Rect| rect.size.x > 0.0 && rect.size.y > 0.0 && rect.contains(point);
    contains(header)
        && !controls.iter().copied().any(contains)
        && !tab_bounds.iter().copied().any(contains)
}

const DOCUMENT_OPEN_TRACE_ENV: &str = "RUDRA_DOCUMENT_OPEN_TRACE";

fn document_open_trace(stage: &str, status: &str, detail: impl AsRef<str>) {
    if std::env::var_os(DOCUMENT_OPEN_TRACE_ENV).is_some() {
        eprintln!(
            "RUDRA_OPEN stage={stage} status={status} {}",
            detail.as_ref()
        );
    }
}

fn pdf_viewer_safety_hint_label(hint: PdfViewerSafetyHint) -> &'static str {
    match hint {
        PdfViewerSafetyHint::JavaScript => "потенциальный JavaScript",
        PdfViewerSafetyHint::Launch => "запуск внешнего файла",
        PdfViewerSafetyHint::ExternalUrl => "внешняя ссылка",
        PdfViewerSafetyHint::EmbeddedFile => "вложенный файл",
        PdfViewerSafetyHint::FileSpecification => "ссылка на файл",
        PdfViewerSafetyHint::GoToRemote => "переход к внешнему документу",
        PdfViewerSafetyHint::OpenAction => "действие при открытии",
    }
}

fn pdf_viewer_safety_detail(hints: &[PdfViewerSafetyHint]) -> Option<String> {
    (!hints.is_empty()).then(|| {
        let labels = hints
            .iter()
            .map(|hint| pdf_viewer_safety_hint_label(*hint))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "PDF открыт в безопасном режиме просмотра: {labels} не выполняются; поддерживаемое содержимое доступно только для чтения."
        )
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Module {
    #[default]
    Home,
    Writer,
    Sheets,
    Pdf,
    Slides,
}

impl Module {
    const fn title(self) -> &'static str {
        match self {
            Self::Home => "Обзор",
            Self::Writer => "Документы",
            Self::Sheets => "Таблицы",
            Self::Pdf => "PDF",
            Self::Slides => "Презентации",
        }
    }

    const fn hint(self) -> &'static str {
        match self {
            Self::Home => "Выберите раздел или откройте документ через меню «Файл».",
            Self::Writer => "DOCX открывается локально только для просмотра без записи.",
            Self::Sheets => "XLSX открывается через IronCalc только для просмотра без записи.",
            Self::Pdf => "PDF проходит проверку и открывается только для просмотра без записи.",
            Self::Slides => "PPTX открывается через Office Oxide только для просмотра без записи.",
        }
    }
}

fn ribbon_tab_widgets() -> [(RibbonTab, &'static [LiveId]); 6] {
    [
        (RibbonTab::Home, ids!(menu_home_button)),
        (RibbonTab::Insert, ids!(menu_insert_button)),
        (RibbonTab::Layout, ids!(menu_layout_button)),
        (RibbonTab::Formulas, ids!(menu_formulas_button)),
        (RibbonTab::Data, ids!(menu_data_button)),
        (RibbonTab::View, ids!(menu_view_button)),
    ]
}

fn ribbon_command_widgets() -> [&'static [LiveId]; 10] {
    [
        ids!(extra_command_0),
        ids!(extra_command_1),
        ids!(extra_command_2),
        ids!(extra_command_3),
        ids!(extra_command_4),
        ids!(extra_command_5),
        ids!(extra_command_6),
        ids!(extra_command_7),
        ids!(extra_command_8),
        ids!(extra_command_9),
    ]
}

#[cfg(windows)]
fn accessibility_module_node_id(module: Module) -> &'static str {
    match module {
        Module::Home => "rudra-office/module/home",
        Module::Writer => "rudra-office/module/writer",
        Module::Sheets => "rudra-office/module/sheets",
        Module::Pdf => "rudra-office/module/pdf",
        Module::Slides => "rudra-office/module/slides",
    }
}

#[cfg(windows)]
fn accessibility_module_from_node_id(node_id: &str) -> Option<Module> {
    [
        Module::Home,
        Module::Writer,
        Module::Sheets,
        Module::Pdf,
        Module::Slides,
    ]
    .into_iter()
    .find(|module| accessibility_module_node_id(*module) == node_id)
}

#[cfg(windows)]
fn accessibility_document_scope(id: RuntimeDocumentId) -> String {
    format!("rudra-office/document:{}", id.get())
}

#[cfg(windows)]
fn accessibility_document_tab_node_id(id: RuntimeDocumentId) -> String {
    format!("{}/tab", accessibility_document_scope(id))
}

#[cfg(windows)]
fn accessibility_document_close_node_id(id: RuntimeDocumentId) -> String {
    format!("{}/tab/close", accessibility_document_scope(id))
}

#[cfg(windows)]
fn accessibility_node(
    window: &AccessibilityWindow,
    id: impl Into<String>,
    role: AccessibilityRole,
    name: impl Into<String>,
    bounds: Rect,
) -> AccessibilityNode {
    AccessibilityNode::new(id, role, name, window.screen_rect(bounds))
}

#[cfg(windows)]
fn accessibility_button(
    window: &AccessibilityWindow,
    id: impl Into<String>,
    name: impl Into<String>,
    bounds: Rect,
    enabled: bool,
) -> AccessibilityNode {
    let mut node = accessibility_node(window, id, AccessibilityRole::Button, name, bounds);
    node.enabled = enabled;
    node.actions = AccessibilityActions {
        invoke: true,
        ..Default::default()
    };
    node
}

#[cfg(windows)]
fn accessibility_selectable(
    window: &AccessibilityWindow,
    id: impl Into<String>,
    role: AccessibilityRole,
    name: impl Into<String>,
    bounds: Rect,
    selected: bool,
) -> AccessibilityNode {
    let mut node = accessibility_node(window, id, role, name, bounds);
    node.selected = Some(selected);
    node.actions = AccessibilityActions {
        select: true,
        ..Default::default()
    };
    node
}

#[cfg(test)]
use app_pdf::PDF_MAX_VISIBLE_PAGES;
use app_pdf::{PDF_MAX_SCALE, PDF_MIN_SCALE, restore_pdf_session_view};
pub use app_pdf::{
    PdfController, PdfFramePage, PdfPageCache, PdfPagePlacement, PdfViewEffect, PdfViewState,
    PdfViewStatus, PdfViewerFrame, PdfZoomMode, RenderedPage, compute_pdf_fit_page_scale,
    compute_pdf_fit_width_scale,
};

/// Maximum number of rows the read-only Sheets viewer projects at once.
const SHEETS_VIEW_MAX_ROWS: usize = 200;
/// Maximum number of columns the read-only Sheets viewer projects at once.
const SHEETS_VIEW_MAX_COLUMNS: usize = 40;
/// Byte cap for a single projected cell string.
const SHEETS_CELL_BYTE_CAP: usize = 512;
/// Mirrors the existing SheetsUi defaults. Bounded XLSX currently does not
/// parse per-row/per-column dimensions, so scroll-to-window conversion must
/// use the same stable default geometry as the renderer.
fn sheets_default_row_height() -> u32 {
    u32::from(sheets_api::DocumentConfig::default().default_row_height)
}

fn sheets_default_column_width() -> u32 {
    u32::from(sheets_api::DocumentConfig::default().default_column_width)
}

/// Plans a source-backed XLSX rectangle around the persisted visual scroll.
/// The overscan keeps ordinary wheel motion inside one resident window while
/// still capping retained cells at the parser boundary.
fn bounded_sheet_window_from_scroll(
    sheet_index: usize,
    scroll_x: u32,
    scroll_y: u32,
) -> BoundedSheetWindow {
    let visible_row = scroll_y / sheets_default_row_height();
    let visible_column = scroll_x / sheets_default_column_width();
    let start_row = visible_row.saturating_sub(BOUNDED_WINDOW_ROWS / 4);
    let start_column = visible_column.saturating_sub(BOUNDED_WINDOW_COLUMNS / 4);
    BoundedSheetWindow::new(
        sheet_index,
        start_row,
        start_column,
        BOUNDED_WINDOW_ROWS,
        BOUNDED_WINDOW_COLUMNS,
        BOUNDED_CELLS_PER_SHEET,
    )
}

/// Converts a zero-based column index into its A1-style letters.
pub fn column_label(column: ColumnIndex) -> String {
    let mut remaining = column.0 as u64 + 1;
    let mut label = String::new();
    while remaining > 0 {
        let rem = ((remaining - 1) % 26) as u8;
        label.insert(0, (b'A' + rem) as char);
        remaining = (remaining - 1) / 26;
    }
    label
}

/// Renders a persisted cell input as read-only display text.
pub fn cell_input_text(input: &CellInput) -> String {
    let text = match input {
        CellInput::Empty => String::new(),
        CellInput::Text(value) => value.clone(),
        CellInput::Number(value) => format_finite_f64(*value),
        CellInput::Percent(value) => format!("{}%", format_finite_f64(*value * 100.0)),
        CellInput::Date(value) => format_finite_f64(*value),
        CellInput::Time(value) => format_finite_f64(*value),
        CellInput::Boolean(value) => if *value { "ИСТИНА" } else { "ЛОЖЬ" }.to_owned(),
        CellInput::Formula(formula) => format!("={formula}"),
    };
    bounded_text(text, SHEETS_CELL_BYTE_CAP)
}

/// Formats an f64 without a trailing `.0` and without panicking on non-finite input.
fn format_finite_f64(value: f64) -> String {
    if !value.is_finite() {
        return "#ЗНАЧ!".to_owned();
    }
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        let mut text = format!("{value}");
        if text.contains('e') || text.contains('E') {
            text = format!("{value:.6}");
        }
        text
    }
}

/// Truncates a string to a byte cap on a char boundary.
fn bounded_text(mut text: String, cap: usize) -> String {
    if text.len() <= cap {
        return text;
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

/// Read-only projection of one sheet into a bounded, dense grid.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetGridView {
    /// Sheet name as stored in the snapshot.
    pub name: String,
    /// Column headers (A1-style) for the projected columns.
    pub column_labels: Vec<String>,
    /// Row headers (1-based) for the projected rows.
    pub row_labels: Vec<String>,
    /// Dense `rows x columns` display strings.
    pub rows: Vec<Vec<String>>,
    /// Total non-empty cells present in the snapshot sheet.
    pub total_cells: usize,
    /// True when the snapshot exceeded the projection caps.
    pub truncated: bool,
}

impl SheetGridView {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() || self.column_labels.is_empty()
    }
}

/// Projects a single sheet snapshot into a bounded read-only grid.
///
/// The projection is pure: it never mutates the snapshot and never allocates
/// beyond [`SHEETS_VIEW_MAX_ROWS`] x [`SHEETS_VIEW_MAX_COLUMNS`] display cells.
pub fn project_sheet_grid(sheet: &SheetSnapshot) -> SheetGridView {
    let total_cells = sheet.cells.len();

    // Collect the occupied coordinates so the grid stays dense and ordered
    // even when the snapshot stores cells sparsely and out of order.
    let mut rows_present: BTreeSet<u32> = BTreeSet::new();
    let mut columns_present: BTreeSet<u32> = BTreeSet::new();
    for cell in &sheet.cells {
        if matches!(cell.input, CellInput::Empty) {
            continue;
        }
        rows_present.insert(cell.address.row.0);
        columns_present.insert(cell.address.column.0);
    }

    let truncated = rows_present.len() > SHEETS_VIEW_MAX_ROWS
        || columns_present.len() > SHEETS_VIEW_MAX_COLUMNS;

    let visible_rows: Vec<u32> = rows_present
        .iter()
        .copied()
        .take(SHEETS_VIEW_MAX_ROWS)
        .collect();
    let visible_columns: Vec<u32> = columns_present
        .iter()
        .copied()
        .take(SHEETS_VIEW_MAX_COLUMNS)
        .collect();

    let row_slot: BTreeMap<u32, usize> = visible_rows
        .iter()
        .enumerate()
        .map(|(slot, row)| (*row, slot))
        .collect();
    let column_slot: BTreeMap<u32, usize> = visible_columns
        .iter()
        .enumerate()
        .map(|(slot, column)| (*column, slot))
        .collect();

    let mut rows = vec![vec![String::new(); visible_columns.len()]; visible_rows.len()];
    for cell in &sheet.cells {
        if matches!(cell.input, CellInput::Empty) {
            continue;
        }
        let (Some(&row), Some(&column)) = (
            row_slot.get(&cell.address.row.0),
            column_slot.get(&cell.address.column.0),
        ) else {
            continue;
        };
        rows[row][column] = cell_input_text(&cell.input);
    }

    SheetGridView {
        name: sheet.name.clone(),
        column_labels: visible_columns
            .iter()
            .map(|column| column_label(ColumnIndex(*column)))
            .collect(),
        row_labels: visible_rows
            .iter()
            .map(|row| (u64::from(*row) + 1).to_string())
            .collect(),
        rows,
        total_cells,
        truncated,
    }
}

/// Resolves the sheet the viewer should display for a snapshot.
///
/// Falls back to the first sheet when `active_sheet` does not match any sheet,
/// keeping the snapshot itself as the untouched source of truth.
pub fn active_sheet_index(snapshot: &WorkbookSnapshot) -> Option<usize> {
    if snapshot.sheets.is_empty() {
        return None;
    }
    snapshot
        .sheets
        .iter()
        .position(|sheet| sheet.id == snapshot.active_sheet)
        .or(Some(0))
}

/// Clamps a requested sheet index into the snapshot's valid range.
pub fn clamp_sheet_index(snapshot: &WorkbookSnapshot, requested: usize) -> Option<usize> {
    if snapshot.sheets.is_empty() {
        None
    } else {
        Some(requested.min(snapshot.sheets.len() - 1))
    }
}

/// Builds the sheet-tab summary line for the viewer header.
pub fn sheet_tabs_label(snapshot: &WorkbookSnapshot, selected: Option<usize>) -> String {
    if snapshot.sheets.is_empty() {
        return "Листы: нет".to_owned();
    }
    let names: Vec<String> = snapshot
        .sheets
        .iter()
        .enumerate()
        .map(|(index, sheet)| {
            if Some(index) == selected {
                format!("[{}]", sheet.name)
            } else {
                sheet.name.clone()
            }
        })
        .collect();
    bounded_text(
        format!("Листы: {}", names.join(" · ")),
        SHEETS_CELL_BYTE_CAP,
    )
}

/// Builds the status line describing the projected grid.
pub fn sheet_grid_status(view: &SheetGridView) -> String {
    if view.is_empty() {
        return format!("Лист «{}»: нет заполненных ячеек.", view.name);
    }
    let base = format!(
        "Лист «{}»: {} строк(и) × {} столбц(а/ов), {} заполненных ячеек.",
        view.name,
        view.rows.len(),
        view.column_labels.len(),
        view.total_cells
    );
    if view.truncated {
        format!("{base} Показан фрагмент (лимит просмотра).")
    } else {
        base
    }
}

/// Renders the projected grid as fixed-width text for the read-only viewer.
pub fn render_grid_text(view: &SheetGridView) -> String {
    if view.is_empty() {
        return String::new();
    }

    let row_header_width = view
        .row_labels
        .iter()
        .map(String::len)
        .max()
        .unwrap_or(1)
        .max(1);

    let mut widths: Vec<usize> = view
        .column_labels
        .iter()
        .map(|label| label.chars().count().max(3))
        .collect();
    for row in &view.rows {
        for (index, cell) in row.iter().enumerate() {
            if let Some(width) = widths.get_mut(index) {
                *width = (*width).max(cell.chars().count()).min(24);
            }
        }
    }

    let mut out = String::new();
    out.push_str(&" ".repeat(row_header_width));
    for (label, width) in view.column_labels.iter().zip(&widths) {
        out.push(' ');
        out.push_str(&pad_cell(label, *width));
    }

    for (row_label, row) in view.row_labels.iter().zip(&view.rows) {
        out.push('\n');
        out.push_str(&format!("{row_label:>row_header_width$}"));
        for (cell, width) in row.iter().zip(&widths) {
            out.push(' ');
            out.push_str(&pad_cell(cell, *width));
        }
    }
    out
}

/// Pads or truncates a cell to a fixed display width, counted in chars.
fn pad_cell(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count > width {
        let truncated: String = text.chars().take(width.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        format!("{text}{}", " ".repeat(width - count))
    }
}

/// Source-model categories retained by the PPTX adapter before a visible slide
/// is converted to a bounded render plan. This is not a renderer capability
/// claim: per-slide warnings and draw reports carry the actual fidelity state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PresentationProjectionCounts {
    objects: usize,
    text: usize,
    images: usize,
    shapes: usize,
    lines: usize,
    tables: usize,
}

fn presentation_projection_counts(presentation: &Presentation) -> PresentationProjectionCounts {
    let mut counts = PresentationProjectionCounts::default();
    for slide in &presentation.slides {
        for object in &slide.objects {
            counts.objects += 1;
            match &object.kind {
                ObjectKind::Text(_) => counts.text += 1,
                ObjectKind::Image(_) => counts.images += 1,
                ObjectKind::Shape(_) => counts.shapes += 1,
                ObjectKind::Line(_) => counts.lines += 1,
                ObjectKind::Table(_) => counts.tables += 1,
            }
        }
    }
    counts
}

#[derive(Default)]
enum OpenDocument {
    #[default]
    None,
    Sheets(WorkbookSnapshot),
    Writer(Box<WriterViewModel>),
    Pdf {
        document_id: DocumentId,
        page_count: u32,
        page_ids: Vec<PageId>,
    },
    Slides(Presentation),
}

struct DocumentOpenJob {
    receiver: Receiver<DocumentOpenResult>,
}

struct DocumentOpenResult {
    path: PathBuf,
    /// Present only for bounded XLSX sessions. Keeping the compressed source
    /// allows later grid windows without eager IronCalc materialization.
    retained_source_bytes: Option<Arc<[u8]>>,
    result: Result<(OpenDocument, Option<PdfOpenArtifact>, String, String), String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlidesViewSnapshot {
    pub current_index: usize,
    pub current_slide_id: Option<SlideId>,
    pub slide_count: usize,
    pub scale: f32,
}

fn slides_navigation_enabled(snapshot: SlidesViewSnapshot) -> (bool, bool) {
    let can_previous = snapshot.current_index > 0 && snapshot.current_index < snapshot.slide_count;
    let can_next = snapshot.current_index.saturating_add(1) < snapshot.slide_count;
    (can_previous, can_next)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WriterHomeCommand {
    Undo,
    Redo,
    SetFontFamily(String),
    SetFontSizePoints(u16),
    ToggleBold,
    ToggleItalic,
    ToggleUnderline,
    ToggleStrikethrough,
    SetForeground(Option<Rgba8>),
    SetBackground(Option<Rgba8>),
    SetAlignment(WriterAlignment),
    SetHeading(Option<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OfficeHistoryCommand {
    Undo,
    Redo,
}

const COLOR_DROPDOWN_PROMPT_INDEX: usize = 0;

fn writer_color_command_from_dropdown(index: usize) -> Option<WriterHomeCommand> {
    Some(match index {
        1 => WriterHomeCommand::SetForeground(None),
        2 => WriterHomeCommand::SetForeground(Some(Rgba8::rgb(31, 41, 55))),
        3 => WriterHomeCommand::SetForeground(Some(Rgba8::rgb(220, 38, 38))),
        4 => WriterHomeCommand::SetForeground(Some(Rgba8::rgb(37, 99, 235))),
        5 => WriterHomeCommand::SetForeground(Some(Rgba8::rgb(22, 163, 74))),
        6 => WriterHomeCommand::SetForeground(Some(Rgba8::rgb(124, 58, 237))),
        7 => WriterHomeCommand::SetBackground(None),
        8 => WriterHomeCommand::SetBackground(Some(Rgba8::rgb(254, 240, 138))),
        9 => WriterHomeCommand::SetBackground(Some(Rgba8::rgb(187, 247, 208))),
        10 => WriterHomeCommand::SetBackground(Some(Rgba8::rgb(186, 230, 253))),
        11 => WriterHomeCommand::SetBackground(Some(Rgba8::rgb(251, 207, 232))),
        12 => WriterHomeCommand::SetBackground(Some(Rgba8::rgb(229, 231, 235))),
        _ => return None,
    })
}

fn writer_color_command_from_accessibility_node(node_id: &str) -> Option<WriterHomeCommand> {
    Some(match node_id {
        "rudra-office/home/writer/text-auto" => WriterHomeCommand::SetForeground(None),
        "rudra-office/home/writer/text-black" => {
            WriterHomeCommand::SetForeground(Some(Rgba8::rgb(31, 41, 55)))
        }
        "rudra-office/home/writer/text-red" => {
            WriterHomeCommand::SetForeground(Some(Rgba8::rgb(220, 38, 38)))
        }
        "rudra-office/home/writer/text-blue" => {
            WriterHomeCommand::SetForeground(Some(Rgba8::rgb(37, 99, 235)))
        }
        "rudra-office/home/writer/text-green" => {
            WriterHomeCommand::SetForeground(Some(Rgba8::rgb(22, 163, 74)))
        }
        "rudra-office/home/writer/text-purple" => {
            WriterHomeCommand::SetForeground(Some(Rgba8::rgb(124, 58, 237)))
        }
        "rudra-office/home/writer/highlight-none" => WriterHomeCommand::SetBackground(None),
        "rudra-office/home/writer/highlight-yellow" => {
            WriterHomeCommand::SetBackground(Some(Rgba8::rgb(254, 240, 138)))
        }
        "rudra-office/home/writer/highlight-green" => {
            WriterHomeCommand::SetBackground(Some(Rgba8::rgb(187, 247, 208)))
        }
        "rudra-office/home/writer/highlight-blue" => {
            WriterHomeCommand::SetBackground(Some(Rgba8::rgb(186, 230, 253)))
        }
        "rudra-office/home/writer/highlight-pink" => {
            WriterHomeCommand::SetBackground(Some(Rgba8::rgb(251, 207, 232)))
        }
        "rudra-office/home/writer/highlight-gray" => {
            WriterHomeCommand::SetBackground(Some(Rgba8::rgb(229, 231, 235)))
        }
        _ => return None,
    })
}

fn writer_color_accessibility_value(command: &WriterHomeCommand) -> Option<&'static str> {
    match command {
        WriterHomeCommand::SetForeground(None) => Some("text-auto"),
        WriterHomeCommand::SetForeground(Some(color)) if *color == Rgba8::rgb(31, 41, 55) => {
            Some("text-black")
        }
        WriterHomeCommand::SetForeground(Some(color)) if *color == Rgba8::rgb(220, 38, 38) => {
            Some("text-red")
        }
        WriterHomeCommand::SetForeground(Some(color)) if *color == Rgba8::rgb(37, 99, 235) => {
            Some("text-blue")
        }
        WriterHomeCommand::SetForeground(Some(color)) if *color == Rgba8::rgb(22, 163, 74) => {
            Some("text-green")
        }
        WriterHomeCommand::SetForeground(Some(color)) if *color == Rgba8::rgb(124, 58, 237) => {
            Some("text-purple")
        }
        WriterHomeCommand::SetBackground(None) => Some("highlight-none"),
        WriterHomeCommand::SetBackground(Some(color)) if *color == Rgba8::rgb(254, 240, 138) => {
            Some("highlight-yellow")
        }
        WriterHomeCommand::SetBackground(Some(color)) if *color == Rgba8::rgb(187, 247, 208) => {
            Some("highlight-green")
        }
        WriterHomeCommand::SetBackground(Some(color)) if *color == Rgba8::rgb(186, 230, 253) => {
            Some("highlight-blue")
        }
        WriterHomeCommand::SetBackground(Some(color)) if *color == Rgba8::rgb(251, 207, 232) => {
            Some("highlight-pink")
        }
        WriterHomeCommand::SetBackground(Some(color)) if *color == Rgba8::rgb(229, 231, 235) => {
            Some("highlight-gray")
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WriterRulerTarget {
    ParagraphFirstLine(u64),
    ParagraphLeft(u64),
    ParagraphRight(u64),
    HeaderDistance,
    FooterDistance,
}

fn writer_ruler_target(marker_id: &str) -> Option<WriterRulerTarget> {
    if marker_id == WRITER_HEADER_DISTANCE_MARKER_ID {
        return Some(WriterRulerTarget::HeaderDistance);
    }
    if marker_id == WRITER_FOOTER_DISTANCE_MARKER_ID {
        return Some(WriterRulerTarget::FooterDistance);
    }
    [
        (
            WRITER_PARAGRAPH_FIRST_LINE_MARKER_PREFIX,
            WriterRulerTarget::ParagraphFirstLine as fn(u64) -> WriterRulerTarget,
        ),
        (
            WRITER_PARAGRAPH_LEFT_MARKER_PREFIX,
            WriterRulerTarget::ParagraphLeft as fn(u64) -> WriterRulerTarget,
        ),
        (
            WRITER_PARAGRAPH_RIGHT_MARKER_PREFIX,
            WriterRulerTarget::ParagraphRight as fn(u64) -> WriterRulerTarget,
        ),
    ]
    .into_iter()
    .find_map(|(prefix, target)| {
        marker_id
            .strip_prefix(prefix)
            .and_then(|value| value.parse::<u64>().ok())
            .map(target)
    })
}

fn writer_indents_from_ruler(
    target: WriterRulerTarget,
    absolute_half_points: i32,
    page_width_half_points: u32,
    margins: writer_api::layout::PageMargins,
    left_half_points: i32,
    right_half_points: i32,
    first_line_half_points: i32,
) -> (i32, i32, i32) {
    let page_width = i64::from(page_width_half_points).max(1);
    let content_start = i64::from(margins.left_half_points).clamp(0, page_width);
    let content_end = page_width
        .saturating_sub(i64::from(margins.right_half_points))
        .clamp(0, page_width);
    let current_left = (content_start + i64::from(left_half_points)).clamp(0, page_width);
    let current_right = (content_end - i64::from(right_half_points)).clamp(0, page_width);
    let minimum_width = 24_i64.min(page_width);
    let marker = i64::from(absolute_half_points).clamp(0, page_width);
    let to_i32 = |value: i64| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;

    match target {
        WriterRulerTarget::ParagraphLeft(_) => {
            let new_left = marker.min(current_right.saturating_sub(minimum_width));
            (
                to_i32(new_left - content_start),
                right_half_points,
                first_line_half_points,
            )
        }
        WriterRulerTarget::ParagraphRight(_) => {
            let new_right = marker.max(current_left.saturating_add(minimum_width));
            (
                left_half_points,
                to_i32(content_end - new_right),
                first_line_half_points,
            )
        }
        WriterRulerTarget::ParagraphFirstLine(_) => {
            let new_first_line = marker.clamp(0, current_right.max(0));
            (
                left_half_points,
                right_half_points,
                to_i32(new_first_line - current_left),
            )
        }
        WriterRulerTarget::HeaderDistance | WriterRulerTarget::FooterDistance => {
            (left_half_points, right_half_points, first_line_half_points)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SheetsHomeFormat {
    ToggleBold,
    ToggleItalic,
    ToggleUnderline,
    AlignLeft,
    AlignCenter,
    AlignRight,
    ToggleWrap,
    SetTextColor(SheetsRgba8),
    SetCellBackground(SheetsRgba8),
}

fn sheets_color_command_from_dropdown(index: usize) -> Option<SheetsHomeFormat> {
    Some(match index {
        1 => SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(31, 41, 55)),
        2 => SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(220, 38, 38)),
        3 => SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(37, 99, 235)),
        4 => SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(22, 163, 74)),
        5 => SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(124, 58, 237)),
        6 => SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(255, 255, 255)),
        7 => SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(254, 240, 138)),
        8 => SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(187, 247, 208)),
        9 => SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(186, 230, 253)),
        10 => SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(251, 207, 232)),
        11 => SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(229, 231, 235)),
        _ => return None,
    })
}

fn sheets_color_command_from_accessibility_node(node_id: &str) -> Option<SheetsHomeFormat> {
    Some(match node_id {
        "rudra-office/home/sheets/text-black" => {
            SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(31, 41, 55))
        }
        "rudra-office/home/sheets/text-red" => {
            SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(220, 38, 38))
        }
        "rudra-office/home/sheets/text-blue" => {
            SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(37, 99, 235))
        }
        "rudra-office/home/sheets/text-green" => {
            SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(22, 163, 74))
        }
        "rudra-office/home/sheets/text-purple" => {
            SheetsHomeFormat::SetTextColor(SheetsRgba8::rgb(124, 58, 237))
        }
        "rudra-office/home/sheets/cell-white" => {
            SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(255, 255, 255))
        }
        "rudra-office/home/sheets/cell-yellow" => {
            SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(254, 240, 138))
        }
        "rudra-office/home/sheets/cell-green" => {
            SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(187, 247, 208))
        }
        "rudra-office/home/sheets/cell-blue" => {
            SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(186, 230, 253))
        }
        "rudra-office/home/sheets/cell-pink" => {
            SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(251, 207, 232))
        }
        "rudra-office/home/sheets/cell-gray" => {
            SheetsHomeFormat::SetCellBackground(SheetsRgba8::rgb(229, 231, 235))
        }
        _ => return None,
    })
}

fn sheets_color_accessibility_value(command: &SheetsHomeFormat) -> Option<&'static str> {
    match command {
        SheetsHomeFormat::SetTextColor(color) if *color == SheetsRgba8::rgb(31, 41, 55) => {
            Some("text-black")
        }
        SheetsHomeFormat::SetTextColor(color) if *color == SheetsRgba8::rgb(220, 38, 38) => {
            Some("text-red")
        }
        SheetsHomeFormat::SetTextColor(color) if *color == SheetsRgba8::rgb(37, 99, 235) => {
            Some("text-blue")
        }
        SheetsHomeFormat::SetTextColor(color) if *color == SheetsRgba8::rgb(22, 163, 74) => {
            Some("text-green")
        }
        SheetsHomeFormat::SetTextColor(color) if *color == SheetsRgba8::rgb(124, 58, 237) => {
            Some("text-purple")
        }
        SheetsHomeFormat::SetCellBackground(color) if *color == SheetsRgba8::rgb(255, 255, 255) => {
            Some("cell-white")
        }
        SheetsHomeFormat::SetCellBackground(color) if *color == SheetsRgba8::rgb(254, 240, 138) => {
            Some("cell-yellow")
        }
        SheetsHomeFormat::SetCellBackground(color) if *color == SheetsRgba8::rgb(187, 247, 208) => {
            Some("cell-green")
        }
        SheetsHomeFormat::SetCellBackground(color) if *color == SheetsRgba8::rgb(186, 230, 253) => {
            Some("cell-blue")
        }
        SheetsHomeFormat::SetCellBackground(color) if *color == SheetsRgba8::rgb(251, 207, 232) => {
            Some("cell-pink")
        }
        SheetsHomeFormat::SetCellBackground(color) if *color == SheetsRgba8::rgb(229, 231, 235) => {
            Some("cell-gray")
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlidesHomeCommand {
    Add,
    Duplicate,
    Delete,
    SetTextColor(SlidesColor),
}

fn slides_color_command_from_dropdown(index: usize) -> Option<SlidesHomeCommand> {
    Some(match index {
        1 => SlidesHomeCommand::SetTextColor(SlidesColor::rgb(31, 41, 55)),
        2 => SlidesHomeCommand::SetTextColor(SlidesColor::rgb(220, 38, 38)),
        3 => SlidesHomeCommand::SetTextColor(SlidesColor::rgb(37, 99, 235)),
        4 => SlidesHomeCommand::SetTextColor(SlidesColor::rgb(22, 163, 74)),
        5 => SlidesHomeCommand::SetTextColor(SlidesColor::rgb(124, 58, 237)),
        _ => return None,
    })
}

fn slides_color_command_from_accessibility_node(node_id: &str) -> Option<SlidesHomeCommand> {
    Some(match node_id {
        "rudra-office/home/slides/text-black" => {
            SlidesHomeCommand::SetTextColor(SlidesColor::rgb(31, 41, 55))
        }
        "rudra-office/home/slides/text-red" => {
            SlidesHomeCommand::SetTextColor(SlidesColor::rgb(220, 38, 38))
        }
        "rudra-office/home/slides/text-blue" => {
            SlidesHomeCommand::SetTextColor(SlidesColor::rgb(37, 99, 235))
        }
        "rudra-office/home/slides/text-green" => {
            SlidesHomeCommand::SetTextColor(SlidesColor::rgb(22, 163, 74))
        }
        "rudra-office/home/slides/text-purple" => {
            SlidesHomeCommand::SetTextColor(SlidesColor::rgb(124, 58, 237))
        }
        _ => return None,
    })
}

fn slides_color_accessibility_value(command: &SlidesHomeCommand) -> Option<&'static str> {
    match command {
        SlidesHomeCommand::SetTextColor(color) if *color == SlidesColor::rgb(31, 41, 55) => {
            Some("text-black")
        }
        SlidesHomeCommand::SetTextColor(color) if *color == SlidesColor::rgb(220, 38, 38) => {
            Some("text-red")
        }
        SlidesHomeCommand::SetTextColor(color) if *color == SlidesColor::rgb(37, 99, 235) => {
            Some("text-blue")
        }
        SlidesHomeCommand::SetTextColor(color) if *color == SlidesColor::rgb(22, 163, 74) => {
            Some("text-green")
        }
        SlidesHomeCommand::SetTextColor(color) if *color == SlidesColor::rgb(124, 58, 237) => {
            Some("text-purple")
        }
        _ => None,
    }
}

fn writer_container_len(
    snapshot: &writer_api::core::DocumentSnapshot,
    container: TextContainerId,
) -> Option<usize> {
    match container {
        TextContainerId::Block(block_id) => snapshot.blocks.iter().find_map(|block| match block {
            BlockSnapshot::Text(text) if text.id == block_id => Some(text.len()),
            _ => None,
        }),
        TextContainerId::Cell(cell_id) => snapshot
            .blocks
            .iter()
            .filter_map(BlockSnapshot::table)
            .flat_map(|table| table.rows.iter())
            .flat_map(|row| row.cells.iter())
            .find(|cell| cell.id == cell_id)
            .map(|cell| cell.text.chars().count()),
    }
}

fn writer_selected_block_id(writer: &WriterViewModel) -> Option<u64> {
    match writer.api().selection().active().container() {
        TextContainerId::Block(block_id) => Some(block_id.get()),
        TextContainerId::Cell(_) => None,
    }
}

fn writer_char_range_from_byte_range(
    text: &str,
    start_byte: usize,
    end_byte: usize,
) -> Result<(usize, usize), String> {
    if start_byte > end_byte {
        return Err("Границы выделения Writer указаны в неверном порядке.".to_owned());
    }
    let start = text
        .get(..start_byte)
        .ok_or_else(|| "Начало выделения Writer не совпадает с границей символа.".to_owned())?
        .chars()
        .count();
    let end = text
        .get(..end_byte)
        .ok_or_else(|| "Конец выделения Writer не совпадает с границей символа.".to_owned())?
        .chars()
        .count();
    Ok((start, end))
}

fn writer_minimal_text_change(previous: &str, current: &str) -> (usize, usize, String) {
    let previous_chars: Vec<char> = previous.chars().collect();
    let current_chars: Vec<char> = current.chars().collect();
    let prefix = previous_chars
        .iter()
        .zip(&current_chars)
        .take_while(|(left, right)| left == right)
        .count();
    let mut suffix = 0;
    while suffix < previous_chars.len().saturating_sub(prefix)
        && suffix < current_chars.len().saturating_sub(prefix)
        && previous_chars[previous_chars.len() - suffix - 1]
            == current_chars[current_chars.len() - suffix - 1]
    {
        suffix += 1;
    }
    let replacement = current_chars[prefix..current_chars.len() - suffix]
        .iter()
        .collect();
    (prefix, previous_chars.len() - suffix, replacement)
}

fn writer_prepare_character_selection(writer: &mut WriterViewModel) -> Result<(), String> {
    let selection = writer.api().selection();
    if !selection.is_collapsed() {
        return Ok(());
    }
    let container = selection.active().container();
    let length = writer_container_len(&writer.snapshot(), container)
        .ok_or_else(|| "Текущий текстовый контейнер Writer не найден.".to_owned())?;
    if length == 0 {
        return Err("Текущий абзац пуст: сначала введите текст.".to_owned());
    }
    writer
        .set_selection(Selection::new(
            Cursor::new(container, TextOffset::new(0)),
            Cursor::new(container, TextOffset::new(length)),
        ))
        .map_err(|error| error.to_string())
}

fn writer_toolbar_result(label: &str, status: ToolbarApplyStatus) -> Result<String, String> {
    match status {
        ToolbarApplyStatus::Applied => Ok(format!("Writer: {label} — применено.")),
        ToolbarApplyStatus::Unchanged => Ok(format!("Writer: {label} — без изменений.")),
        ToolbarApplyStatus::Disabled => Err(format!(
            "Writer: команда «{label}» недоступна для текущего выделения."
        )),
    }
}

const WRITER_TEXT_EDITOR_PROMPT_INDEX: usize = 0;
const WRITER_TEXT_EDITOR_MAX_BLOCKS: usize = 128;
const WRITER_TEXT_EDITOR_LABEL_CHAR_CAP: usize = 56;
const WRITER_ZOOM_PRESET_PROMPT_INDEX: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
struct WriterTextEditorBlock {
    block_id: u64,
    label: String,
    text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct WriterTextEditorDraft {
    selected_block_id: Option<u64>,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WriterTextEditorModel {
    labels: Vec<String>,
    selected_index: usize,
    text: String,
    enabled: bool,
}

fn writer_text_editor_preview(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "(пустой)".to_owned();
    }
    let mut preview = compact
        .chars()
        .take(WRITER_TEXT_EDITOR_LABEL_CHAR_CAP)
        .collect::<String>();
    if compact.chars().count() > WRITER_TEXT_EDITOR_LABEL_CHAR_CAP {
        preview.push('…');
    }
    preview
}

fn writer_text_editor_blocks(writer: &WriterViewModel) -> Vec<WriterTextEditorBlock> {
    writer
        .snapshot()
        .blocks
        .iter()
        .filter_map(BlockSnapshot::text_block)
        .take(WRITER_TEXT_EDITOR_MAX_BLOCKS)
        .enumerate()
        .map(|(index, block)| WriterTextEditorBlock {
            block_id: block.id.get(),
            label: format!(
                "Абзац {}: {}",
                index + 1,
                writer_text_editor_preview(block.text())
            ),
            text: block.text.clone(),
        })
        .collect()
}

fn writer_text_editor_labels(blocks: &[WriterTextEditorBlock]) -> Vec<String> {
    if blocks.is_empty() {
        return vec!["Нет текстовых абзацев".to_owned()];
    }
    let mut labels = Vec::with_capacity(blocks.len() + 1);
    labels.push("Выберите абзац…".to_owned());
    labels.extend(blocks.iter().map(|block| block.label.clone()));
    labels
}

fn writer_zoom_dropdown_index_for_percent(percent: u16) -> usize {
    match percent {
        50 => 0,
        75 => 1,
        100 => 2,
        125 => 3,
        150 => 4,
        200 => 5,
        _ => WRITER_ZOOM_PRESET_PROMPT_INDEX,
    }
}

fn writer_zoom_percent_from_dropdown(index: usize) -> Option<u16> {
    match index {
        0 => Some(50),
        1 => Some(75),
        2 => Some(100),
        3 => Some(125),
        4 => Some(150),
        5 => Some(200),
        _ => None,
    }
}

fn writer_zoom_percent_from_value(value: &str) -> Result<u16, String> {
    let percent = value
        .trim()
        .trim_end_matches('%')
        .trim()
        .parse::<u16>()
        .map_err(|_| {
            "Масштаб Writer должен быть числом: 50%, 75%, 100%, 125%, 150% или 200%.".to_owned()
        })?;
    match percent {
        50 | 75 | 100 | 125 | 150 | 200 => Ok(percent),
        _ => Err("Выберите масштаб Writer: 50%, 75%, 100%, 125%, 150% или 200%.".to_owned()),
    }
}

#[derive(Default)]
struct AppState {
    // Used only while no document is active. As soon as a session is active,
    // its typed identity and kind are the single source of truth for the UI.
    idle_module: Module,
    status: String,
    detail: String,
    documents: DocumentRegistry,
    active_document: Option<RuntimeDocumentId>,
    // Format-specific fields below adapt the one visible Makepad view; the
    // canonical per-document values live in DocumentSession::view_state.
    sheets_selected: usize,
    /// The source-backed rectangular range currently materialized in the
    /// active large workbook. It is transient UI state; document zoom/scroll
    /// remain owned by DocumentSession::view_state.
    sheets_bounded_window: Option<BoundedSheetWindow>,
    sheets_api: Option<SpreadsheetApi<IroncalcAdapter>>,
    sheets_view: Option<SheetsUi>,
    /// Last measured non-zero Makepad grid size. The pure Sheets UI receives
    /// this size when it binds a document and whenever the canvas is resized.
    sheets_viewport: Option<(u32, u32)>,
    /// Navigation/search drafts are isolated by typed document identity and sheet.
    sheets_tools: BTreeMap<(RuntimeDocumentId, SheetId), SheetsToolsDraft>,
    sheets_clipboard: SheetsClipboardState<ClipboardPayload>,
    slides_current_index: usize,
    slides_scale: f32,
    slides_viewer: Option<PresentationViewer>,
    slides_presentation_mode: bool,
    pdf_engine: PdfOxideAdapter,
    pdf_controller: PdfController,
    pdf_effect: PdfViewEffect,
    pdf_loading: bool,
    pdf_worker: Option<PdfRenderWorker>,
    pdf_raster_batch: Option<app_pdf::PdfRasterBatch>,
    pdf_request_sequence: u64,
    pdf_raster_generation: u64,
    open_job: Option<DocumentOpenJob>,
    pending_open_path: Option<PathBuf>,
    pending_blank_creation: Option<(PathBuf, BlankDocumentFormat)>,
    new_document_formats: BTreeMap<RuntimeDocumentId, BlankDocumentFormat>,
    new_document_temp_paths: BTreeMap<RuntimeDocumentId, PathBuf>,
    file_menu_open: bool,
    home_ribbon_open: bool,
    ribbon_tab: RibbonTab,
    ribbon_command_feedback: Option<String>,
    /// Per-tab, uncommitted content of the Writer paragraph editor. The
    /// document model remains the source of truth after Apply; this map only
    /// prevents an in-progress draft in one tab from replacing another tab's
    /// input field during UI refresh.
    writer_text_drafts: BTreeMap<RuntimeDocumentId, WriterTextEditorDraft>,
    writer_inline_session: Option<WriterInlineEditorSession>,
    writer_inline_epoch: u64,
    writer_color_value: Option<&'static str>,
    sheets_color_value: Option<&'static str>,
    slides_color_value: Option<&'static str>,
    pdf_error: Option<String>,
    writer_display_mode: WriterDisplayMode,
}

impl Drop for AppState {
    fn drop(&mut self) {
        if let Some(worker) = self.pdf_worker.as_ref() {
            worker.shutdown();
        }
        if let Some((path, _)) = self.pending_blank_creation.take() {
            let _ = std::fs::remove_file(path);
        }
        for (_, path) in std::mem::take(&mut self.new_document_temp_paths) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WriterInlineEditorSession {
    document_id: RuntimeDocumentId,
    block_id: u64,
    revision: writer_api::core::Revision,
    epoch: u64,
}

type SheetsCellInputBinding = (RuntimeDocumentId, CellAddress, String);

/// A redraw of the same cell must not replace the editor's uncommitted text.
/// Changing the document, cell or committed value starts a new binding.
fn sheets_input_sync_value(
    previous: &mut Option<SheetsCellInputBinding>,
    next: Option<SheetsCellInputBinding>,
) -> Option<String> {
    if *previous == next {
        return None;
    }
    let raw = next
        .as_ref()
        .map(|(_, _, raw)| raw.clone())
        .unwrap_or_default();
    *previous = next;
    Some(raw)
}

#[derive(Script, ScriptHook)]
pub struct App {
    #[live]
    ui: WidgetRef,
    #[rust(AppState::default())]
    state: AppState,
    #[rust]
    launcher_startup_request: Option<rudra_office_launcher::LaunchRequest>,
    #[rust]
    writer_inline_editor: Option<WriterInlineEditorSession>,
    #[rust]
    writer_inline_editor_pending_focus: bool,
    #[rust]
    sheets_cell_input_binding: Option<SheetsCellInputBinding>,
    #[rust]
    native_paste_hook: NativeGridPasteSlot,
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WriterDisplayMode {
    #[default]
    Visual,
    PlainText,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriterFallback {
    Unavailable,
    Empty,
    Text,
}

const WRITER_FALLBACK_BYTE_CAP: usize = 12 * 1024;
const WRITER_VIEW_MAX_PAGES: usize = 200;
const WRITER_VIEW_MAX_BLOCKS_PER_PAGE: usize = 400;
const WRITER_VIEW_MAX_FRAGMENTS_PER_BLOCK: usize = 800;
const WRITER_FRAGMENT_BYTE_CAP: usize = 1024;

#[cfg(test)]
pub struct WriterPageInfo {
    pub page_count: usize,
    pub mode_label: String,
}

fn writer_projection_notice(vm: &WriterViewModel) -> Option<String> {
    writer_projection_notice_from_diagnostics(vm.docx_projection())
}

fn writer_projection_notice_from_diagnostics(
    diagnostics: &writer_api::DocxProjectionDiagnostics,
) -> Option<String> {
    let fallback_count = diagnostics.fallback.total();
    (fallback_count > 0).then(|| {
        format!("Часть содержимого DOCX отображена текстовыми заметками: {fallback_count}.")
    })
}

fn writer_page_setup_label(vm: &WriterViewModel) -> String {
    let settings = vm.api().layout_settings();
    let name = match (
        settings.page_size.width_half_points,
        settings.page_size.height_half_points,
    ) {
        (1191, 1684) => "A4",
        (1684, 2381) => "A3",
        (839, 1191) => "A5",
        (1417, 2004) => "B4",
        (998, 1417) => "B5",
        (1224, 1584) => "Letter",
        (1224, 2016) => "Legal",
        _ => "Custom",
    };
    let orientation =
        if settings.page_size.width_half_points > settings.page_size.height_half_points {
            "Landscape"
        } else {
            "Portrait"
        };
    let selected_chars = vm
        .api()
        .selection()
        .range()
        .map(|(_, range)| range.end().get().saturating_sub(range.start().get()))
        .unwrap_or(0);
    if selected_chars > 0 {
        format!("{name} | {orientation} · Выделено: {selected_chars} симв.")
    } else {
        format!("{name} | {orientation}")
    }
}
#[cfg(test)]
fn writer_page_info(vm: &WriterViewModel) -> WriterPageInfo {
    let state = vm.state();
    WriterPageInfo {
        page_count: state.pages.len(),
        mode_label: match state.mode {
            writer_ui::ViewMode::Pages => "Страницы",
            writer_ui::ViewMode::Continuous => "Непрерывный",
        }
        .to_string(),
    }
}

fn writer_visible_pages_label(range: Option<&std::ops::Range<usize>>, page_count: usize) -> String {
    let Some(range) = range else {
        return format!("Область: — из {page_count}");
    };
    if range.start >= range.end || range.start >= page_count {
        return format!("Область: — из {page_count}");
    }
    let first = range.start + 1;
    let last = range.end.min(page_count);
    if first == last {
        format!("Область: {first} из {page_count}")
    } else {
        format!("Область: {first}–{last} из {page_count}")
    }
}

pub fn writer_zoom_label(state: &writer_ui::ViewState) -> String {
    format!("{}%", state.zoom_percent)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriterFragmentView {
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriterBlockView {
    pub fragments: Vec<WriterFragmentView>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriterPageView {
    pub blocks: Vec<WriterBlockView>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriterTextView {
    pub pages: Vec<WriterPageView>,
    pub truncated: bool,
}

impl WriterTextView {
    pub fn is_empty(&self) -> bool {
        !writer_fragments_have_user_content(self.pages.iter().flat_map(|page| {
            page.blocks.iter().flat_map(|block| {
                block
                    .fragments
                    .iter()
                    .map(|fragment| fragment.text.as_str())
            })
        }))
    }
}

/// A structural empty block is not user content. Non-empty text, including
/// whitespace, is retained as user content because Writer preserves it verbatim.
fn writer_fragments_have_user_content<'a>(mut fragments: impl Iterator<Item = &'a str>) -> bool {
    fragments.any(|text| !text.is_empty())
}

fn bounded_writer_text(mut text: String, cap: usize) -> String {
    if text.len() <= cap {
        return text;
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

/// Projects the writer layout into a bounded read-only text view while
/// preserving page, block, and fragment order.
pub fn project_writer_text(vm: &WriterViewModel) -> Option<WriterTextView> {
    let state = vm.state();
    let Some(layout) = &state.layout else {
        return None;
    };

    let mut total_pages = 0usize;
    let mut total_blocks = 0usize;
    let mut total_fragments = 0usize;
    for page in &layout.pages {
        if !page.blocks.is_empty() {
            total_pages += 1;
            total_blocks += page.blocks.len();
            for block in &page.blocks {
                total_fragments += block.fragments.len();
            }
        }
    }

    let truncated = total_pages > WRITER_VIEW_MAX_PAGES
        || total_blocks > WRITER_VIEW_MAX_PAGES * WRITER_VIEW_MAX_BLOCKS_PER_PAGE
        || total_fragments
            > WRITER_VIEW_MAX_PAGES
                * WRITER_VIEW_MAX_BLOCKS_PER_PAGE
                * WRITER_VIEW_MAX_FRAGMENTS_PER_BLOCK;

    let mut pages = Vec::new();
    for page in layout.pages.iter().take(WRITER_VIEW_MAX_PAGES) {
        if page.blocks.is_empty() {
            continue;
        }
        let mut blocks = Vec::new();
        for block in page.blocks.iter().take(WRITER_VIEW_MAX_BLOCKS_PER_PAGE) {
            let mut fragments = Vec::new();
            for fragment in block
                .fragments
                .iter()
                .take(WRITER_VIEW_MAX_FRAGMENTS_PER_BLOCK)
            {
                fragments.push(WriterFragmentView {
                    text: bounded_writer_text(fragment.text.clone(), WRITER_FRAGMENT_BYTE_CAP),
                });
            }
            blocks.push(WriterBlockView { fragments });
        }
        pages.push(WriterPageView { blocks });
    }

    Some(WriterTextView { pages, truncated })
}

pub fn writer_text_view_label(view: &WriterTextView) -> String {
    if view.is_empty() {
        return "Writer не содержит видимого текста.".to_owned();
    }
    let mut page_count = 0usize;
    let mut block_count = 0usize;
    let mut fragment_count = 0usize;
    for page in &view.pages {
        page_count += 1;
        for block in &page.blocks {
            block_count += 1;
            fragment_count += block.fragments.len();
        }
    }
    let base = format!(
        "Проекция Writer: {} стр., {} блоков, {} фрагментов.",
        page_count, block_count, fragment_count
    );
    if view.truncated {
        format!("{base} Показан фрагмент (лимит просмотра).")
    } else {
        base
    }
}

pub fn writer_fallback_kind(vm: &WriterViewModel) -> WriterFallback {
    let state = vm.state();
    let Some(layout) = &state.layout else {
        return WriterFallback::Unavailable;
    };

    let has_user_content =
        writer_fragments_have_user_content(layout.pages.iter().flat_map(|page| {
            page.blocks.iter().flat_map(|block| {
                block
                    .fragments
                    .iter()
                    .map(|fragment| fragment.text.as_str())
            })
        }));

    if !has_user_content {
        return WriterFallback::Empty;
    }
    WriterFallback::Text
}

pub fn writer_text_fallback(vm: &WriterViewModel) -> Option<String> {
    if writer_fallback_kind(vm) != WriterFallback::Text {
        return None;
    }

    let view = project_writer_text(vm)?;
    let mut out = String::new();
    for page in &view.pages {
        for block in &page.blocks {
            for fragment in &block.fragments {
                if fragment.text.is_empty() {
                    continue;
                }
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&fragment.text);
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(bounded_unicode_text(out))
    }
}

pub fn bounded_unicode_text(mut text: String) -> String {
    if text.len() <= WRITER_FALLBACK_BYTE_CAP {
        return text;
    }

    let mut end = WRITER_FALLBACK_BYTE_CAP;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

pub fn writer_fallback_label(vm: &WriterViewModel) -> String {
    match writer_fallback_kind(vm) {
        WriterFallback::Unavailable => "Ошибка отображения Writer.".to_owned(),
        WriterFallback::Empty => "Writer не содержит видимого текста.".to_owned(),
        WriterFallback::Text => {
            writer_text_fallback(vm).unwrap_or_else(|| "Нет текста для отображения.".to_owned())
        }
    }
}

const SLIDES_VIEW_TEXT_BYTE_CAP: usize = 12 * 1024;

pub fn slides_page_count(presentation: &Presentation) -> usize {
    presentation.slides.len()
}

pub fn clamp_slides_index(presentation: &Presentation, requested: usize) -> Option<usize> {
    if presentation.slides.is_empty() {
        None
    } else {
        Some(requested.min(presentation.slides.len() - 1))
    }
}

pub fn slide_ids(presentation: &Presentation) -> Vec<SlideId> {
    presentation.slides.iter().map(|slide| slide.id).collect()
}

fn bounded_slides_text(mut text: String, cap: usize) -> String {
    if text.len() <= cap {
        return text;
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

fn slide_layout_label(layout: &SlideLayout) -> &'static str {
    match layout {
        SlideLayout::Blank => "Пустой",
        SlideLayout::Title => "Заголовок",
        SlideLayout::TitleAndContent => "Заголовок и содержимое",
        SlideLayout::TwoColumns => "Две колонки",
        SlideLayout::Custom => "Пользовательский",
    }
}

pub fn slide_preview_text(presentation: &Presentation, index: usize) -> Option<String> {
    let slide = presentation.slides.get(index)?;
    let mut text = slide_layout_label(&slide.layout).to_owned();
    if text == "Пустой" {
        text = slide.notes.clone().unwrap_or_default();
        if !text.is_empty() {
            text = text.trim().to_owned();
        }
    }
    if text.is_empty() {
        return None;
    }
    Some(bounded_slides_text(text, SLIDES_VIEW_TEXT_BYTE_CAP))
}

pub fn slides_status_text(presentation: &Presentation, current_index: usize) -> String {
    let count = slides_page_count(presentation);
    if count == 0 {
        "Презентация не содержит слайдов.".to_owned()
    } else {
        format!("Слайд {}/{}", current_index + 1, count)
    }
}

#[cfg(test)]
mod slides_projection_tests {
    use super::*;
    use slides_api::{
        Image, ImageFormat, Line, ObjectId, Point, Rect as SlideRect, Shape, SlideObject, Table,
        Text, Transform,
    };
    use slides_api::{PresentationId, Slide, SlideId, SlideLayout};

    #[test]
    fn slide_count_matches_presentation_length() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        assert_eq!(slides_page_count(&presentation), 0);

        presentation
            .add_slide(Slide::new(SlideId::new(1)))
            .expect("first slide must be valid");
        presentation
            .add_slide(Slide::new(SlideId::new(2)))
            .expect("second slide must be valid");
        assert_eq!(slides_page_count(&presentation), 2);
    }

    #[test]
    fn clamp_slides_index_never_exceeds_range() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        presentation
            .add_slide(Slide::new(SlideId::new(1)))
            .expect("first slide must be valid");
        presentation
            .add_slide(Slide::new(SlideId::new(2)))
            .expect("second slide must be valid");

        assert_eq!(clamp_slides_index(&presentation, 0), Some(0));
        assert_eq!(clamp_slides_index(&presentation, 1), Some(1));
        assert_eq!(clamp_slides_index(&presentation, 99), Some(1));
        assert_eq!(
            clamp_slides_index(&Presentation::new(PresentationId::new(1)), 0),
            None
        );
    }

    #[test]
    fn slide_ids_preserves_insertion_order() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        let first = SlideId::new(1);
        let second = SlideId::new(2);
        presentation
            .add_slide(Slide::new(first))
            .expect("first slide");
        presentation
            .add_slide(Slide::new(second))
            .expect("second slide");

        assert_eq!(slide_ids(&presentation), vec![first, second]);
    }

    #[test]
    fn slide_preview_text_returns_none_for_empty_slide() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        let slide = Slide::new(SlideId::new(1));
        presentation.add_slide(slide).expect("slide must be valid");

        assert_eq!(slide_preview_text(&presentation, 0), None);
    }

    #[test]
    fn slide_preview_text_prefers_layout_name_over_notes() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        let mut slide = Slide::new(SlideId::new(1));
        slide.layout = SlideLayout::Title;
        slide.notes = Some("Notes".to_owned());
        presentation.add_slide(slide).expect("slide must be valid");

        assert_eq!(
            slide_preview_text(&presentation, 0),
            Some("Заголовок".to_owned())
        );
    }

    #[test]
    fn slide_preview_text_falls_back_to_notes_for_blank_layout() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        let mut slide = Slide::new(SlideId::new(1));
        slide.notes = Some("Notes".to_owned());
        presentation.add_slide(slide).expect("slide must be valid");

        assert_eq!(
            slide_preview_text(&presentation, 0),
            Some("Notes".to_owned())
        );
    }

    #[test]
    fn slide_preview_text_bounds_unicode_notes() {
        let mut presentation = Presentation::new(PresentationId::new(1));
        let mut slide = Slide::new(SlideId::new(1));
        slide.layout = SlideLayout::Blank;
        slide.notes = Some("α".repeat(20_000));
        presentation.add_slide(slide).expect("slide must be valid");

        let text = slide_preview_text(&presentation, 0).expect("notes must be non-empty");
        assert!(text.len() <= SLIDES_VIEW_TEXT_BYTE_CAP);
        assert!(text.ends_with('α'));
    }

    #[test]
    fn pptx_accounting_keeps_supported_object_kinds_out_of_unsupported_bucket() {
        let mut presentation = Presentation::new(PresentationId::new(9));
        let mut slide = Slide::new(SlideId::new(1));
        let bounds = SlideRect::new(Point::ZERO, 100.0, 40.0);
        let mut table = Table::new(1, 1).expect("table dimensions are valid");
        table.cells[0] = Some(Text::plain("Ячейка"));

        for (id, kind) in [
            (ObjectId::new(1), ObjectKind::Text(Text::plain("Текст"))),
            (
                ObjectId::new(2),
                ObjectKind::Image(Image {
                    bytes: vec![1],
                    format: ImageFormat::Png,
                    alt_text: Some("Логотип".to_owned()),
                }),
            ),
            (ObjectId::new(3), ObjectKind::Shape(Shape::default())),
            (
                ObjectId::new(4),
                ObjectKind::Line(Line {
                    start: Point::ZERO,
                    end: Point::new(100.0, 40.0),
                    ..Line::default()
                }),
            ),
            (ObjectId::new(5), ObjectKind::Table(table)),
        ] {
            slide
                .add_object(SlideObject::new(id, kind, bounds, Transform::IDENTITY))
                .expect("supported model object must be retained");
        }
        presentation
            .add_slide(slide)
            .expect("slide with supported objects must be valid");

        assert_eq!(
            presentation_projection_counts(&presentation),
            PresentationProjectionCounts {
                objects: 5,
                text: 1,
                images: 1,
                shapes: 1,
                lines: 1,
                tables: 1,
            }
        );
    }

    fn runtime_test_session(path: &str, backend: OpenDocument) -> DocumentSession {
        DocumentSession::new(DocumentSource::from_path(PathBuf::from(path)), backend)
            .expect("test backend must form a valid document session")
    }

    fn runtime_test_writer() -> OpenDocument {
        OpenDocument::Writer(Box::new(WriterViewModel::new()))
    }

    fn runtime_test_slides(presentation_id: u64, slide_count: u64) -> OpenDocument {
        let mut presentation = Presentation::new(PresentationId::new(presentation_id));
        for index in 1..=slide_count {
            presentation
                .add_slide(Slide::new(SlideId::new(index)))
                .expect("test slide must be valid");
        }
        OpenDocument::Slides(presentation)
    }

    #[test]
    fn document_registry_supports_all_office_kinds_and_duplicate_paths() {
        let mut registry = DocumentRegistry::default();
        assert!(registry.is_empty());

        let writer_a = registry
            .insert(runtime_test_session("same.docx", runtime_test_writer()))
            .expect("Writer session must register");
        let writer_b = registry
            .insert(runtime_test_session("same.docx", runtime_test_writer()))
            .expect("duplicate path must create a distinct session");
        let sheets = registry
            .insert(runtime_test_session(
                "book.xlsx",
                OpenDocument::Sheets(WorkbookSnapshot::default()),
            ))
            .expect("Sheets session must register");
        let pdf_backend_id = DocumentId::new(77);
        let pdf = registry
            .insert(runtime_test_session(
                "document.pdf",
                OpenDocument::Pdf {
                    document_id: pdf_backend_id,
                    page_count: 1,
                    page_ids: vec![PageId::new(1)],
                },
            ))
            .expect("PDF session must register");
        let presentation_backend_id = PresentationId::new(88);
        let slides = registry
            .insert(runtime_test_session(
                "deck.pptx",
                OpenDocument::Slides(Presentation::new(presentation_backend_id)),
            ))
            .expect("Slides session must register");

        assert_ne!(writer_a, writer_b);
        assert_eq!(registry.len(), 5);
        assert!(registry.contains(writer_a));
        assert!(registry.contains(writer_b));
        assert_eq!(
            registry
                .iter()
                .map(|(id, session)| (id.get(), session.kind()))
                .collect::<Vec<_>>(),
            vec![
                (writer_a.get(), Module::Writer),
                (writer_b.get(), Module::Writer),
                (sheets.get(), Module::Sheets),
                (pdf.get(), Module::Pdf),
                (slides.get(), Module::Slides),
            ]
        );
        assert_eq!(
            registry.require(writer_a).unwrap().source().path(),
            registry.require(writer_b).unwrap().source().path()
        );

        assert_eq!(
            registry.require(pdf).unwrap().backend_kind(),
            document_runtime::BackendHandleKind::Pdf
        );
        match registry.require(pdf).unwrap().backend() {
            OpenDocument::Pdf { document_id, .. } => assert_eq!(*document_id, pdf_backend_id),
            _ => panic!("PDF backend must remain inside its session"),
        }
        match registry.require(slides).unwrap().backend() {
            OpenDocument::Slides(presentation) => {
                assert_eq!(presentation.id, presentation_backend_id)
            }
            _ => panic!("Slides backend must remain inside its session"),
        }

        let sheets_backend = SpreadsheetApi::new(IroncalcAdapter::new());
        let sheets_token = sheets_backend.document().token();
        assert!(sheets_token > 0);
        assert_ne!(
            std::any::type_name::<RuntimeDocumentId>(),
            std::any::type_name::<DocumentId>()
        );
        assert_ne!(
            std::any::type_name::<RuntimeDocumentId>(),
            std::any::type_name::<PresentationId>()
        );
        assert_ne!(
            std::any::type_name::<RuntimeDocumentId>(),
            std::any::type_name::<u64>()
        );
        assert_ne!(
            std::any::type_name::<RuntimeDocumentId>(),
            std::any::type_name::<WidgetUid>()
        );
        assert_ne!(
            std::any::type_name::<RuntimeDocumentId>(),
            std::any::type_name::<LiveId>()
        );
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RuntimeDocumentId>();

        let removed = registry.remove(writer_a).expect("live ID must remove");
        assert_eq!(removed.kind(), Module::Writer);
        assert!(matches!(
            registry.require(writer_a),
            Err(DocumentRuntimeError::EntityNotFound(id)) if id == writer_a
        ));
        let next = registry
            .insert(runtime_test_session("next.docx", runtime_test_writer()))
            .expect("allocation after removal must succeed");
        assert!(next.get() > slides.get());
        assert!(!registry.contains(writer_a));
    }

    #[test]
    fn document_runtime_smoke_switch_target_zoom_and_close() {
        let mut state = AppState::default();
        let document_a = state
            .register_document(PathBuf::from("same.docx"), runtime_test_writer())
            .expect("document A must register");
        let document_b = state
            .register_document(PathBuf::from("same.docx"), runtime_test_writer())
            .expect("document B must register");

        assert_eq!(state.documents.len(), 2);
        assert_ne!(document_a, document_b);
        assert_eq!(format!("document:{document_a}"), "document:1");

        state
            .activate_document(document_a)
            .expect("document A must activate");
        assert_eq!(state.active_document_id(), Some(document_a));
        assert_eq!(
            state
                .zoom_active_document_to(1.25)
                .expect("active zoom must route through its ID"),
            1.25
        );

        assert_eq!(
            state
                .set_zoom(document_b, 2.0)
                .expect("inactive target zoom must work"),
            2.0
        );
        assert_eq!(
            state
                .scroll(document_b, 12.0, 34.0)
                .expect("inactive target scroll must work"),
            (12.0, 34.0)
        );
        state
            .set_view_mode(
                document_b,
                DocumentViewMode::WriterLayout(ViewMode::Continuous),
            )
            .expect("inactive target view mode must work");
        assert_eq!(state.active_document_id(), Some(document_a));

        let inspection_a = state.inspect(document_a).expect("A must inspect");
        let inspection_b = state.inspect(document_b).expect("B must inspect");
        let DocumentViewState::Writer(view_a) = inspection_a.view_state else {
            panic!("A must have Writer view state");
        };
        let DocumentViewState::Writer(view_b) = inspection_b.view_state else {
            panic!("B must have Writer view state");
        };
        assert_eq!(view_a.zoom, 1.25);
        assert_eq!(view_b.zoom, 2.0);
        assert_eq!((view_b.scroll_x, view_b.scroll_y), (12.0, 34.0));
        assert_eq!(view_b.layout_mode, ViewMode::Continuous);
        assert_eq!(inspection_b.id, document_b);
        assert_eq!(inspection_b.kind, Module::Writer);
        assert_eq!(
            inspection_b.source_path.as_deref(),
            Some(Path::new("same.docx"))
        );
        assert_eq!(
            inspection_b.backend_kind,
            document_runtime::BackendHandleKind::Writer
        );

        state
            .activate_document(document_b)
            .expect("document B must activate");
        assert_eq!(state.active_document_id(), Some(document_b));
        let DocumentViewState::Writer(restored_b) = state.inspect(document_b).unwrap().view_state
        else {
            panic!("B must remain Writer");
        };
        assert_eq!(restored_b.zoom, 2.0);
        assert_eq!(restored_b.layout_mode, ViewMode::Continuous);

        state
            .close_document(document_a)
            .expect("inactive A must close");
        assert!(!state.documents.contains(document_a));
        assert!(state.documents.contains(document_b));
        assert_eq!(state.active_document_id(), Some(document_b));
        assert_eq!(
            state.inspect(document_a),
            Err(DocumentRuntimeError::EntityNotFound(document_a))
        );

        state
            .close_document(document_b)
            .expect("active B must close");
        assert!(state.documents.is_empty());
        assert_eq!(state.active_document_id(), None);
        assert_eq!(state.active_module(), Module::Home);
        assert_eq!(
            state.close_document(document_b),
            Err(DocumentRuntimeError::EntityNotFound(document_b))
        );

        let document_c = state
            .register_document(PathBuf::from("same.docx"), runtime_test_writer())
            .expect("new document after close must register");
        assert!(document_c.get() > document_b.get());
    }

    #[test]
    fn view_commands_are_explicitly_targeted_and_typed() {
        let mut state = AppState::default();
        let writer = state
            .register_document(PathBuf::from("document.docx"), runtime_test_writer())
            .unwrap();
        let sheets = state
            .register_document(
                PathBuf::from("book.xlsx"),
                OpenDocument::Sheets(WorkbookSnapshot::default()),
            )
            .unwrap();
        let pdf = state
            .register_document(
                PathBuf::from("document.pdf"),
                OpenDocument::Pdf {
                    document_id: DocumentId::new(700),
                    page_count: 2,
                    page_ids: vec![PageId::new(1), PageId::new(2)],
                },
            )
            .unwrap();
        let slides = state
            .register_document(PathBuf::from("deck.pptx"), runtime_test_slides(800, 2))
            .unwrap();

        assert_eq!(state.set_zoom(writer, 1.5).unwrap(), 1.5);
        assert_eq!(state.set_zoom(sheets, 1.75).unwrap(), 1.75);
        assert_eq!(state.set_zoom(pdf, 2.0).unwrap(), 2.0);
        assert_eq!(state.set_zoom(slides, 1.25).unwrap(), 1.25);
        assert_eq!(state.scroll(writer, 5.0, 7.0).unwrap(), (5.0, 7.0));
        assert_eq!(state.scroll(sheets, 9.0, 11.0).unwrap(), (9.0, 11.0));
        assert_eq!(state.scroll(pdf, 4.0, 6.0).unwrap(), (4.0, 6.0));
        state.go_to_page(pdf, 2).unwrap();
        state.go_to_page(slides, 2).unwrap();
        state
            .set_view_mode(writer, DocumentViewMode::WriterLayout(ViewMode::Continuous))
            .unwrap();
        state
            .set_view_mode(
                writer,
                DocumentViewMode::WriterDisplay(WriterDisplayMode::PlainText),
            )
            .unwrap();
        state
            .set_view_mode(pdf, DocumentViewMode::PdfZoom(PdfZoomMode::FitWidth))
            .unwrap();
        state
            .set_view_mode(slides, DocumentViewMode::SlidesPresentation(true))
            .unwrap();

        let DocumentViewState::Writer(writer_view) = state.inspect(writer).unwrap().view_state
        else {
            panic!("Writer view expected");
        };
        assert_eq!(writer_view.zoom, 1.5);
        assert_eq!((writer_view.scroll_x, writer_view.scroll_y), (5.0, 7.0));
        assert_eq!(writer_view.layout_mode, ViewMode::Continuous);
        assert_eq!(writer_view.display_mode, WriterDisplayMode::PlainText);

        let DocumentViewState::Sheets(sheets_view) = state.inspect(sheets).unwrap().view_state
        else {
            panic!("Sheets view expected");
        };
        assert_eq!(sheets_view.zoom, 1.75);
        assert_eq!((sheets_view.scroll_x, sheets_view.scroll_y), (9, 11));

        let DocumentViewState::Pdf(pdf_view) = state.inspect(pdf).unwrap().view_state else {
            panic!("PDF view expected");
        };
        assert_eq!(pdf_view.zoom, 2.0);
        assert_eq!((pdf_view.scroll_x, pdf_view.scroll_y), (4.0, 6.0));
        assert_eq!(pdf_view.page_number, 2);
        assert_eq!(pdf_view.zoom_mode, PdfZoomMode::FitWidth);

        let DocumentViewState::Slides(slides_view) = state.inspect(slides).unwrap().view_state
        else {
            panic!("Slides view expected");
        };
        assert_eq!(slides_view.zoom, 1.25);
        assert_eq!(slides_view.current_index, 1);
        assert!(slides_view.presentation_mode);

        assert_eq!(
            state.scroll(slides, 1.0, 1.0),
            Err(DocumentRuntimeError::UnsupportedOperation {
                id: slides,
                operation: DocumentOperation::Scroll,
                kind: Module::Slides,
            })
        );
        assert_eq!(
            state.go_to_page(writer, 1),
            Err(DocumentRuntimeError::UnsupportedOperation {
                id: writer,
                operation: DocumentOperation::GoToPage,
                kind: Module::Writer,
            })
        );
        assert_eq!(
            state.set_view_mode(sheets, DocumentViewMode::PdfZoom(PdfZoomMode::FitPage)),
            Err(DocumentRuntimeError::UnsupportedOperation {
                id: sheets,
                operation: DocumentOperation::SetViewMode,
                kind: Module::Sheets,
            })
        );
    }

    #[test]
    fn close_releases_real_pdf_backends_and_runtime_cache() {
        let pdf_bytes = match WriterApi::new()
            .export(writer_api::core::DocumentFormat::Pdf)
            .expect("test PDF must export")
        {
            writer_api::ExportedDocument::Pdf(bytes) => bytes,
            _ => panic!("Writer returned an unexpected export type"),
        };
        let mut state = AppState::default();

        let (backend_a, _, _) =
            OpenDocument::open(Path::new("same.pdf"), &pdf_bytes, &mut state.pdf_engine)
                .expect("PDF A must open");
        let pdf_backend_a = backend_a.pdf_bindings().unwrap().0;
        let document_a = state
            .register_document(PathBuf::from("same.pdf"), backend_a)
            .expect("PDF A session must register");
        state
            .activate_document(document_a)
            .expect("PDF A must activate");

        let (backend_b, _, _) =
            OpenDocument::open(Path::new("same.pdf"), &pdf_bytes, &mut state.pdf_engine)
                .expect("PDF B must open");
        let pdf_backend_b = backend_b.pdf_bindings().unwrap().0;
        let document_b = state
            .register_document(PathBuf::from("same.pdf"), backend_b)
            .expect("PDF B session must register");
        state
            .activate_document(document_b)
            .expect("PDF B must activate");

        assert_ne!(document_a, document_b);
        assert_ne!(pdf_backend_a, pdf_backend_b);
        assert_eq!(state.pdf_engine.document_count(), 2);
        assert_eq!(state.documents.len(), 2);
        assert_eq!(
            state.pdf_controller.state().document_id,
            Some(pdf_backend_b)
        );

        state
            .close_document(document_a)
            .expect("inactive PDF A must close");
        assert_eq!(state.pdf_engine.document_count(), 1);
        assert!(!state.documents.contains(document_a));
        assert!(state.documents.contains(document_b));
        assert_eq!(state.active_document_id(), Some(document_b));
        assert!(
            state
                .pdf_controller
                .cache
                .entries
                .iter()
                .all(|entry| entry.key.0 != pdf_backend_a)
        );

        state
            .close_document(document_b)
            .expect("active PDF B must close");
        assert_eq!(state.pdf_engine.document_count(), 0);
        assert!(state.documents.is_empty());
        assert!(state.pdf_controller.cache.is_empty());
        assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Empty);
        assert_eq!(state.active_document_id(), None);
    }

    #[test]
    fn failed_parse_creates_no_registry_orphan_and_preserves_active_identity() {
        let mut state = AppState::default();
        let original = state
            .register_document(PathBuf::from("original.docx"), runtime_test_writer())
            .expect("original session must register");
        state
            .activate_document(original)
            .expect("original session must activate");
        let before_len = state.documents.len();

        let result = OpenDocument::open(
            Path::new("broken.docx"),
            b"not-a-real-docx",
            &mut state.pdf_engine,
        );

        assert!(result.is_err());
        assert_eq!(state.documents.len(), before_len);
        assert_eq!(state.active_document_id(), Some(original));
        assert_eq!(state.inspect(original).unwrap().id, original);
    }
    #[test]
    fn activation_failure_restores_previous_active_document() {
        let mut state = AppState::default();
        let previous = state
            .register_document(PathBuf::from("previous.docx"), runtime_test_writer())
            .expect("previous session must register");
        state
            .activate_document(previous)
            .expect("previous session must activate");

        let failing = state
            .register_document(PathBuf::from("failing.docx"), runtime_test_writer())
            .expect("failing session must register");
        let session = state
            .documents
            .require_mut(failing)
            .expect("failing session must remain addressable");
        let DocumentViewState::Writer(view) = session.view_state_mut() else {
            panic!("failing session must have Writer view state");
        };
        view.zoom = f64::NAN;

        let mut app = App {
            ui: WidgetRef::default(),
            state,
            writer_inline_editor: None,
            writer_inline_editor_pending_focus: false,
            sheets_cell_input_binding: None,
            native_paste_hook: Default::default(),
            launcher_startup_request: None,
        };
        let error = app
            .activate_document(&mut Cx::new(Box::new(|_, _| {})), failing)
            .expect_err("invalid target binding must fail");

        assert!(matches!(error, DocumentRuntimeError::BackendError(_)));
        assert_eq!(app.state.active_document_id(), Some(previous));
        assert_eq!(app.state.active_module(), Module::Writer);
        assert_eq!(app.state.documents.len(), 2);
        assert!(app.state.documents.contains(previous));
        assert!(app.state.documents.contains(failing));
        assert_eq!(app.state.inspect(previous).unwrap().id, previous);
    }
}
