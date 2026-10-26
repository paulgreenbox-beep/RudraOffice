//! Renderer-neutral spreadsheet UI controller and view model.

#[cfg(test)]
use sheets_api::{CellInput, SpreadsheetApi};
use sheets_api::{CellView, SheetSummary};
use sheets_core::{CellFormat, ColumnDimension, RowDimension};
use sheets_grid::{
    CellAddress, CellRange, ColumnIndex, FrozenPanes, GridDimensions, GridError, GridGeometry,
    Point, Rect, RowIndex, ScrollOffset, SheetId, Viewport, VirtualGrid,
};
use std::collections::BTreeSet;

#[cfg(test)]
mod hidden_rows_tests;

#[cfg(test)]
mod hidden_columns_tests;
use std::fmt;

pub type UiResult<T> = Result<T, UiError>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiError {
    Grid(GridError),
    InvalidConfig(&'static str),
    InvalidState(&'static str),
    InvalidAddress(String),
    InvalidInput(String),
}
impl From<GridError> for UiError {
    fn from(v: GridError) -> Self {
        Self::Grid(v)
    }
}
impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Grid(e) => write!(f, "{e}"),
            Self::InvalidConfig(s) => write!(f, "invalid config: {s}"),
            Self::InvalidState(s) => write!(f, "invalid state: {s}"),
            Self::InvalidAddress(s) => write!(f, "invalid address: {s}"),
            Self::InvalidInput(s) => write!(f, "invalid input: {s}"),
        }
    }
}
impl std::error::Error for UiError {}

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
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiTheme {
    pub canvas: Rgba8,
    pub chrome: Rgba8,
    pub grid_line: Rgba8,
    pub text: Rgba8,
    pub muted_text: Rgba8,
    pub selection: Rgba8,
    pub focus: Rgba8,
    pub error: Rgba8,
}
impl Default for UiTheme {
    fn default() -> Self {
        Self {
            canvas: Rgba8::rgb(255, 255, 255),
            chrome: Rgba8::rgb(246, 247, 249),
            grid_line: Rgba8::rgb(218, 221, 226),
            text: Rgba8::rgb(31, 35, 41),
            muted_text: Rgba8::rgb(92, 99, 112),
            selection: Rgba8::rgb(221, 235, 255),
            focus: Rgba8::rgb(30, 104, 210),
            error: Rgba8::rgb(181, 35, 42),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiMetrics {
    pub name_box_width: u32,
    pub formula_bar_height: u32,
    pub column_header_height: u32,
    pub row_header_width: u32,
    pub sheet_tabs_height: u32,
    pub status_bar_height: u32,
    pub formatting_panel_width: u32,
    pub scrollbar_size: u32,
}
impl Default for UiMetrics {
    fn default() -> Self {
        Self {
            name_box_width: 96,
            formula_bar_height: 32,
            column_header_height: 24,
            row_header_width: 48,
            sheet_tabs_height: 32,
            status_bar_height: 24,
            formatting_panel_width: 240,
            scrollbar_size: 14,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct UiConfig {
    pub initial_sheet: SheetId,
    pub dimensions: GridDimensions,
    pub outer_width: u32,
    pub outer_height: u32,
    pub default_row_height: u32,
    pub default_column_width: u32,
    pub frozen: FrozenPanes,
    pub metrics: UiMetrics,
    pub theme: UiTheme,
}
impl Default for UiConfig {
    fn default() -> Self {
        let document_config = sheets_api::DocumentConfig::default();
        Self {
            initial_sheet: SheetId(0),
            dimensions: GridDimensions {
                rows: 100_000,
                columns: 16_384,
            },
            outer_width: 1280,
            outer_height: 800,
            default_row_height: u32::from(document_config.default_row_height),
            default_column_width: u32::from(document_config.default_column_width),
            frozen: FrozenPanes::default(),
            metrics: UiMetrics::default(),
            theme: UiTheme::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiCell {
    pub address: CellAddress,
    pub display: String,
    pub raw: String,
    pub format: Option<CellFormat>,
}
impl From<CellView> for UiCell {
    fn from(v: CellView) -> Self {
        Self {
            address: v.address,
            display: v.display_text(),
            raw: v.raw_text(),
            format: v.format,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiSheet {
    pub id: SheetId,
    pub name: String,
    pub active: bool,
    pub used_cells: u64,
    pub merges: u64,
}
impl From<SheetSummary> for UiSheet {
    fn from(v: SheetSummary) -> Self {
        Self {
            id: v.id,
            name: v.name,
            active: v.active,
            used_cells: v.used_cells,
            merges: v.merges,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct UiSnapshot {
    pub active_sheet: SheetId,
    pub dimensions: GridDimensions,
    pub cells: Vec<UiCell>,
    pub sheets: Vec<UiSheet>,
    pub status_text: String,
    pub frozen: FrozenPanes,
}
impl UiSnapshot {
    pub fn from_api<E>(
        api: &mut sheets_api::SpreadsheetApi<E>,
        xlsx: &[u8],
    ) -> sheets_api::ApiResult<Self>
    where
        E: sheets_api::SpreadsheetEngine,
    {
        api.load_xlsx(xlsx)?;
        Self::from_api_document(api)
    }

    pub fn from_api_document<E>(api: &sheets_api::SpreadsheetApi<E>) -> sheets_api::ApiResult<Self>
    where
        E: sheets_api::SpreadsheetEngine,
    {
        let active_sheet = api.document().active_sheet();
        let snapshot = api.document().snapshot();
        let sheets = snapshot
            .sheets
            .into_iter()
            .map(|sheet| UiSheet {
                id: sheet.id,
                name: sheet.name,
                active: sheet.id == active_sheet,
                used_cells: sheet.cells.len() as u64,
                merges: sheet.merges.len() as u64,
            })
            .collect();
        let cells = api
            .cell_views(active_sheet)?
            .into_iter()
            .map(|view| {
                let mut cell: UiCell = view.into();
                cell.address.sheet = active_sheet;
                cell
            })
            .collect();
        Ok(Self {
            active_sheet,
            dimensions: GridDimensions {
                rows: api.config().max_document_rows,
                columns: api.config().max_document_columns,
            },
            cells,
            sheets,
            status_text: api.status_summary().to_string(),
            frozen: FrozenPanes::default(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusTarget {
    #[default]
    Grid,
    NameBox,
    FormulaBar,
    FormattingPanel,
    SheetTabs,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionKind {
    #[default]
    None,
    Cell,
    Range,
    Row,
    Column,
    Sheet,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionState {
    pub kind: SelectionKind,
    pub anchor: Option<CellAddress>,
    pub focus: Option<CellAddress>,
    /// Canonical bounds, including entire intersecting merged regions. Anchor
    /// and focus retain the user's directional intent inside these bounds.
    /// `None` supports endpoint-only snapshots; restore computes their extent.
    pub extent: Option<CellRange>,
}
impl SelectionState {
    pub fn range(&self) -> Option<CellRange> {
        let anchor = self.anchor?;
        let focus = self.focus?;
        if anchor.sheet != focus.sheet {
            return None;
        }
        if let Some(extent) = &self.extent {
            return (extent.contains(&anchor) && extent.contains(&focus)).then(|| extent.clone());
        }
        let min_row = RowIndex(anchor.row.0.min(focus.row.0));
        let max_row = RowIndex(anchor.row.0.max(focus.row.0));
        let min_col = ColumnIndex(anchor.column.0.min(focus.column.0));
        let max_col = ColumnIndex(anchor.column.0.max(focus.column.0));
        Some(CellRange {
            sheet: anchor.sheet,
            start: CellAddress::new(anchor.sheet, min_row, min_col),
            end: CellAddress::new(anchor.sheet, max_row, max_col),
        })
    }
    pub fn contains(&self, a: CellAddress) -> bool {
        self.range()
            .map(|r| {
                a.sheet == r.sheet
                    && a.row.0 >= r.start.row.0
                    && a.row.0 <= r.end.row.0
                    && a.column.0 >= r.start.column.0
                    && a.column.0 <= r.end.column.0
            })
            .unwrap_or(false)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditSource {
    Grid,
    Keyboard,
    FormulaBar,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditSession {
    pub address: CellAddress,
    pub original: String,
    pub draft: String,
    pub source: EditSource,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormattingPanelState {
    pub open: bool,
    pub enabled: bool,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    Enter,
    Escape,
    Backspace,
    Delete,
    F2,
    Character(char),
}
#[derive(Clone, Debug, PartialEq)]
/// Pointer points use physical pixels local to the complete UiFrame canvas,
/// including headers and the formula bar. SheetsUi applies inverse zoom.
pub enum UiEvent {
    Resize {
        width: u32,
        height: u32,
    },
    PointerDown {
        point: Point,
        button: PointerButton,
        modifiers: Modifiers,
    },
    PointerMove {
        point: Point,
        modifiers: Modifiers,
    },
    /// Adapter must send this (or call cancel_pointer_selection) on capture
    /// loss, canvas leave, or document switching.
    PointerLeave,
    PointerUp {
        point: Point,
        button: PointerButton,
        modifiers: Modifiers,
    },
    PointerDoubleClick {
        point: Point,
        button: PointerButton,
    },
    Wheel {
        delta_x: i64,
        delta_y: i64,
    },
    KeyDown {
        key: Key,
        modifiers: Modifiers,
    },
    TextInput(String),
    FormulaBarInput(String),
    FormulaBarCommit,
    FormulaBarCancel,
    NameBoxCommit(String),
    ActivateSheet(SheetId),
    AddSheet(Option<String>),
    RenameSheet {
        sheet: SheetId,
        name: String,
    },
    DeleteSheet(SheetId),
    DuplicateSheet {
        sheet: SheetId,
        name: Option<String>,
    },
    MoveSheet {
        sheet: SheetId,
        target_index: usize,
    },
    ToggleFormattingPanel,
    ApplyFormat(FormatPatch),
    SetFrozenPanes(FrozenPanes),
}
#[derive(Clone, Debug, PartialEq)]
pub enum UiEffect {
    CommitCellRaw {
        address: CellAddress,
        raw: String,
    },
    ClearRange(CellRange),
    ActivateSheet(SheetId),
    AddSheet(Option<String>),
    RenameSheet {
        sheet: SheetId,
        name: String,
    },
    DeleteSheet(SheetId),
    DuplicateSheet {
        sheet: SheetId,
        name: Option<String>,
    },
    MoveSheet {
        sheet: SheetId,
        target_index: usize,
    },
    ApplyFormat {
        range: CellRange,
        patch: FormatPatch,
    },
    SetFrozenPanes(FrozenPanes),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiLayout {
    pub name_box: Rect,
    pub formula_bar: Rect,
    pub corner: Rect,
    pub column_header: Rect,
    pub row_header: Rect,
    pub grid_viewport: Rect,
    pub horizontal_scrollbar: Rect,
    pub vertical_scrollbar: Rect,
    pub sheet_tabs: Rect,
    pub status_bar: Rect,
    pub formatting_panel: Option<Rect>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CellRenderModel {
    pub address: CellAddress,
    pub rect: Rect,
    /// Clip for the cell's frozen/body pane; None means overscan only.
    /// Renderers keep rect for content placement and clip to visible_rect.
    pub visible_rect: Option<Rect>,
    pub display: String,
    pub selected: bool,
    pub active: bool,
    pub editing: bool,
    pub format: Option<CellFormat>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct HeaderRenderModel {
    pub index: u32,
    pub label: String,
    pub rect: Rect,
    /// Clip excludes body headers scrolled underneath frozen headers.
    pub visible_rect: Option<Rect>,
    pub selected: bool,
}
/// A viewport-intersecting merge with complete geometry and anchor cell data.
#[derive(Clone, Debug, PartialEq)]
pub struct UiMergedCell {
    pub range: CellRange,
    pub anchor: UiCell,
    /// Full projected range, including the portion outside the viewport.
    pub rect: Rect,
    /// The visible intersection used for clipping and hit testing.
    pub visible_rect: Rect,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetTabRenderModel {
    pub id: SheetId,
    pub name: String,
    pub active: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct UiFrame {
    pub layout: UiLayout,
    pub cells: Vec<CellRenderModel>,
    pub row_headers: Vec<HeaderRenderModel>,
    pub column_headers: Vec<HeaderRenderModel>,
    pub tabs: Vec<SheetTabRenderModel>,
    pub selection: SelectionState,
    pub editor: Option<EditSession>,
    pub name_box: String,
    pub formula_text: String,
    pub status_text: String,
    pub formatting_panel: FormattingPanelState,
    pub content_width: f64,
    pub scroll_x: f64,
    pub scroll_y: f64,
    pub content_height: f64,
    pub zoom: f32,
    pub merges: Vec<CellRange>,
    pub merged_cells: Vec<UiMergedCell>,
}

pub struct SheetsUi {
    config: UiConfig,
    grid: VirtualGrid<UiCell>,
    snapshot: UiSnapshot,
    selection: SelectionState,
    // Provenance for recomputing merge closure after metadata changes. Public
    // selection, rendering and document effects use SelectionState.extent.
    selection_seed: Option<CellRange>,
    edit: Option<EditSession>,
    focus: FocusTarget,
    scroll: ScrollOffset,
    layout: UiLayout,
    dragging: bool,
    formula_draft: String,
    panel: FormattingPanelState,
    merges: Vec<CellRange>,
    zoom: f32,
}

impl SheetsUi {
    pub fn new(config: UiConfig) -> UiResult<Self> {
        if config.default_row_height == 0 {
            return Err(UiError::InvalidConfig(
                "default row height must be non-zero",
            ));
        }
        if config.default_column_width == 0 {
            return Err(UiError::InvalidConfig(
                "default column width must be non-zero",
            ));
        }
        if config.outer_width == 0 || config.outer_height == 0 {
            return Err(UiError::InvalidConfig("outer dimensions must be non-zero"));
        }
        let mut grid = VirtualGrid::new(
            config.initial_sheet,
            config.dimensions,
            config.default_row_height,
            config.default_column_width,
        )
        .map_err(UiError::from)?;
        grid.set_frozen_panes(config.frozen);
        let snapshot = UiSnapshot {
            active_sheet: config.initial_sheet,
            dimensions: config.dimensions,
            cells: Vec::new(),
            sheets: Vec::new(),
            status_text: String::new(),
            frozen: config.frozen,
        };
        let layout = compute_layout(&config, false);
        Ok(Self {
            config,
            grid,
            snapshot,
            selection: SelectionState::default(),
            selection_seed: None,
            edit: None,
            focus: FocusTarget::Grid,
            scroll: ScrollOffset::default(),
            layout,
            dragging: false,
            formula_draft: String::new(),
            panel: FormattingPanelState::default(),
            merges: Vec::new(),
            zoom: 1.0,
        })
    }

    pub fn sync(&mut self, snapshot: UiSnapshot) -> UiResult<()> {
        let mut next_grid = VirtualGrid::new(
            snapshot.active_sheet,
            snapshot.dimensions,
            self.config.default_row_height,
            self.config.default_column_width,
        )?;
        next_grid.set_frozen_panes(snapshot.frozen);
        if snapshot.active_sheet == self.active_sheet() {
            let hidden: Vec<_> = self
                .grid
                .hidden_rows()
                .filter(|row| row.0 < snapshot.dimensions.rows)
                .collect();
            next_grid.set_hidden_rows(&hidden)?;
            let hidden_columns: Vec<_> = self
                .grid
                .hidden_columns()
                .filter(|column| column.0 < snapshot.dimensions.columns)
                .collect();
            next_grid.set_hidden_columns(&hidden_columns)?;
        }
        for cell in &snapshot.cells {
            if cell.address.sheet != snapshot.active_sheet {
                return Err(UiError::InvalidAddress(
                    "cell belongs to a different sheet".into(),
                ));
            }
            snapshot.dimensions.validate_address(cell.address)?;
            next_grid.insert_cell(cell.address, cell.clone())?;
        }
        let changed = self.snapshot.active_sheet != snapshot.active_sheet
            || self.snapshot.dimensions != snapshot.dimensions;
        self.config.initial_sheet = snapshot.active_sheet;
        self.config.dimensions = snapshot.dimensions;
        self.config.frozen = snapshot.frozen;
        self.grid = next_grid;
        self.merges.clear();
        self.snapshot = snapshot;
        if changed {
            self.cancel_pointer_selection();
            self.selection = SelectionState::default();
            self.selection_seed = None;
            self.edit = None;
            self.scroll = ScrollOffset::default();
        }
        self.layout = compute_layout(&self.config, self.panel.open);
        Ok(())
    }

    pub fn handle_event(&mut self, event: UiEvent) -> UiResult<Vec<UiEffect>> {
        Ok(match event {
            UiEvent::Resize { width, height } => {
                self.cancel_pointer_selection();
                self.config.outer_width = width;
                self.config.outer_height = height;
                self.layout = compute_layout(&self.config, self.panel.open);
                if let Some(address) = self.selection.focus {
                    self.ensure_visible(address);
                }
                self.clamp_scroll();
                Vec::new()
            }
            UiEvent::PointerDown {
                point,
                button,
                modifiers,
            } => self.handle_pointer_down(point, button, modifiers),
            UiEvent::PointerMove { point, modifiers } => self.handle_pointer_move(point, modifiers),
            UiEvent::PointerLeave => {
                self.cancel_pointer_selection();
                Vec::new()
            }
            UiEvent::PointerUp {
                point,
                button,
                modifiers,
            } => self.handle_pointer_up(point, button, modifiers),
            UiEvent::PointerDoubleClick { point, button: _ } => self.handle_double_click(point),
            UiEvent::Wheel { delta_x, delta_y } => self.handle_wheel(delta_x, delta_y),
            UiEvent::KeyDown { key, modifiers } => self.handle_key(key, modifiers),
            UiEvent::TextInput(text) => self.handle_text_input(text),
            UiEvent::FormulaBarInput(text) => {
                self.formula_draft = text.clone();
                Vec::new()
            }
            UiEvent::FormulaBarCommit => self.commit_formula_bar(),
            UiEvent::FormulaBarCancel => {
                self.formula_draft.clear();
                self.edit = None;
                Vec::new()
            }
            UiEvent::NameBoxCommit(text) => self.commit_name_box(text),
            UiEvent::ActivateSheet(sheet) => self.activate_sheet(sheet),
            UiEvent::AddSheet(name) => vec![UiEffect::AddSheet(name)],
            UiEvent::RenameSheet { sheet, name } => vec![UiEffect::RenameSheet { sheet, name }],
            UiEvent::DeleteSheet(sheet) => vec![UiEffect::DeleteSheet(sheet)],
            UiEvent::DuplicateSheet { sheet, name } => {
                vec![UiEffect::DuplicateSheet { sheet, name }]
            }
            UiEvent::MoveSheet {
                sheet,
                target_index,
            } => vec![UiEffect::MoveSheet {
                sheet,
                target_index,
            }],
            UiEvent::ToggleFormattingPanel => {
                self.cancel_pointer_selection();
                self.panel.open = !self.panel.open;
                self.layout = compute_layout(&self.config, self.panel.open);
                Vec::new()
            }
            UiEvent::ApplyFormat(patch) => self.apply_format(patch),
            UiEvent::SetFrozenPanes(frozen) => {
                self.cancel_pointer_selection();
                let clamped = FrozenPanes {
                    rows: frozen.rows.min(self.config.dimensions.rows),
                    cols: frozen.cols.min(self.config.dimensions.columns),
                };
                self.config.frozen = clamped;
                self.grid.set_frozen_panes(clamped);
                self.layout = compute_layout(&self.config, self.panel.open);
                vec![UiEffect::SetFrozenPanes(clamped)]
            }
        })
    }

    /// Returns the grid viewport in document coordinates. The frame is drawn
    /// in physical pixels after this layout step, so virtualized rows must be
    /// selected from the inverse-scaled viewport rather than the raw canvas.
    fn logical_grid_viewport(&self) -> Viewport {
        let zoom = self.zoom.clamp(0.5, 2.0) as f64;
        Viewport::new(
            0.0,
            0.0,
            (self.layout.grid_viewport.w / zoom).ceil(),
            (self.layout.grid_viewport.h / zoom).ceil(),
        )
    }

    pub fn frame(&self) -> UiResult<UiFrame> {
        let layout = self.grid.layout(self.logical_grid_viewport(), self.scroll);
        let mut rows = BTreeSet::new();
        let mut columns = BTreeSet::new();
        for range in [layout.frozen_rows, layout.body_rows].into_iter().flatten() {
            for row in self.grid.visible_rows(range) {
                rows.insert(row.0);
            }
        }
        for range in [layout.frozen_cols, layout.body_cols].into_iter().flatten() {
            for column in self.grid.visible_columns(range) {
                columns.insert(column.0);
            }
        }

        let zoom = self.zoom as f64;
        let grid_origin = self.layout.grid_viewport;
        let mut cells = Vec::new();
        for &row in &rows {
            for &col in &columns {
                let address =
                    CellAddress::new(self.snapshot.active_sheet, RowIndex(row), ColumnIndex(col));
                let Some(doc) = self.grid.cell_rect(address) else {
                    continue;
                };
                let base_rect = Rect::new(
                    self.layout.grid_viewport.x
                        + if col < self.config.frozen.cols {
                            doc.x
                        } else {
                            doc.x - self.scroll.x as f64
                        },
                    self.layout.grid_viewport.y
                        + if row < self.config.frozen.rows {
                            doc.y
                        } else {
                            doc.y - self.scroll.y as f64
                        },
                    doc.w,
                    doc.h,
                );
                let rect = zoom_rect(base_rect, grid_origin, zoom);
                let display = self
                    .grid
                    .cell(address)
                    .map(|cell| cell.display.clone())
                    .unwrap_or_default();
                cells.push(CellRenderModel {
                    address,
                    rect,
                    visible_rect: rect.intersection(self.cell_pane(address)),
                    display,
                    selected: self.selection.contains(address),
                    active: self.focus == FocusTarget::Grid
                        && self.active_sheet() == address.sheet
                        && self.selection.focus == Some(address),
                    editing: self.edit.as_ref().map(|e| e.address) == Some(address),
                    format: self.grid.cell(address).and_then(|cell| cell.format.clone()),
                });
            }
        }

        let mut row_headers = Vec::new();
        for &row in &rows {
            let address =
                CellAddress::new(self.snapshot.active_sheet, RowIndex(row), ColumnIndex(0));
            let Some(doc) = self.grid.cell_rect(address) else {
                continue;
            };
            let base_y = self.layout.grid_viewport.y
                + if row < self.config.frozen.rows {
                    doc.y
                } else {
                    doc.y - self.scroll.y as f64
                };
            let y = grid_origin.y + (base_y - grid_origin.y) * zoom;
            let rect = Rect::new(
                self.layout.row_header.x,
                y,
                self.layout.row_header.w,
                doc.h * zoom,
            );
            let pane = self.cell_pane(address);
            row_headers.push(HeaderRenderModel {
                index: row,
                label: (row + 1).to_string(),
                rect,
                visible_rect: rect.intersection(Rect::new(
                    self.layout.row_header.x,
                    pane.y,
                    self.layout.row_header.w,
                    pane.h,
                )),
                selected: self
                    .selection
                    .focus
                    .map(|focus| focus.row.0 == row)
                    .unwrap_or(false),
            });
        }

        let mut column_headers = Vec::new();
        for &col in &columns {
            let address =
                CellAddress::new(self.snapshot.active_sheet, RowIndex(0), ColumnIndex(col));
            let Some(doc) = self.grid.cell_rect(address) else {
                continue;
            };
            let base_x = self.layout.grid_viewport.x
                + if col < self.config.frozen.cols {
                    doc.x
                } else {
                    doc.x - self.scroll.x as f64
                };
            let x = grid_origin.x + (base_x - grid_origin.x) * zoom;
            let rect = Rect::new(
                x,
                self.layout.column_header.y,
                doc.w * zoom,
                self.layout.column_header.h,
            );
            let pane = self.cell_pane(address);
            column_headers.push(HeaderRenderModel {
                index: col,
                label: col_to_label(col),
                rect,
                visible_rect: rect.intersection(Rect::new(
                    pane.x,
                    self.layout.column_header.y,
                    pane.w,
                    self.layout.column_header.h,
                )),
                selected: self
                    .selection
                    .focus
                    .map(|focus| focus.column.0 == col)
                    .unwrap_or(false),
            });
        }

        let merged_cells = self
            .merges
            .iter()
            .filter_map(|range| {
                let doc = self.grid.range_rect(range)?;
                let address = range.start;
                let base_rect = Rect::new(
                    grid_origin.x
                        + if address.column.0 < self.config.frozen.cols {
                            doc.x
                        } else {
                            doc.x - self.scroll.x as f64
                        },
                    grid_origin.y
                        + if address.row.0 < self.config.frozen.rows {
                            doc.y
                        } else {
                            doc.y - self.scroll.y as f64
                        },
                    doc.w,
                    doc.h,
                );
                let rect = zoom_rect(base_rect, grid_origin, zoom);
                let visible_rect = rect.intersection(grid_origin)?;
                let anchor = self.grid.cell(address).cloned().unwrap_or_else(|| UiCell {
                    address,
                    display: String::new(),
                    raw: String::new(),
                    format: None,
                });
                Some(UiMergedCell {
                    range: range.clone(),
                    anchor,
                    rect,
                    visible_rect,
                })
            })
            .collect();

        Ok(UiFrame {
            layout: self.layout,
            cells,
            row_headers,
            column_headers,
            tabs: self
                .snapshot
                .sheets
                .iter()
                .map(|s| SheetTabRenderModel {
                    id: s.id,
                    name: s.name.clone(),
                    active: s.active,
                })
                .collect(),
            selection: self.selection.clone(),
            editor: self.edit.clone(),
            name_box: self
                .selection
                .focus
                .map(|focus| format!("{}{}", col_to_label(focus.column.0), focus.row.0 + 1))
                .unwrap_or_default(),
            formula_text: if self.formula_draft.is_empty() {
                self.active_input_address()
                    .and_then(|focus| self.grid.cell(focus).map(|cell| cell.raw.clone()))
                    .unwrap_or_default()
            } else {
                self.formula_draft.clone()
            },
            status_text: self.snapshot.status_text.clone(),
            formatting_panel: self.panel.clone(),
            content_width: (layout.content_width as f64 * zoom).max(0.0),
            scroll_x: self.scroll.x as f64 * zoom,
            scroll_y: self.scroll.y as f64 * zoom,
            content_height: (layout.content_height as f64 * zoom).max(0.0),
            zoom: self.zoom,
            merges: self.merges.clone(),
            merged_cells,
        })
    }

    pub fn selection(&self) -> &SelectionState {
        &self.selection
    }

    /// Input reads/writes target the top-left of a merged region, while
    /// selection.focus keeps the user's logical pointer/keyboard endpoint.
    pub fn active_input_address(&self) -> Option<CellAddress> {
        self.selection
            .focus
            .map(|address| self.merged_anchor(address))
    }

    /// Adapters must report focus changes before forwarding keyboard events.
    /// Text-field shortcuts stay with the text field while grid focus is absent.
    pub fn set_focus_target(&mut self, focus: FocusTarget) {
        if focus != FocusTarget::Grid {
            self.cancel_pointer_selection();
        }
        self.focus = focus;
    }
    pub fn edit_session(&self) -> Option<&EditSession> {
        self.edit.as_ref()
    }
    pub fn scroll_offset(&self) -> ScrollOffset {
        self.scroll
    }
    pub fn active_sheet(&self) -> SheetId {
        self.snapshot.active_sheet
    }
    pub fn theme(&self) -> &UiTheme {
        &self.config.theme
    }
    pub fn layout(&self) -> &UiLayout {
        &self.layout
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.cancel_pointer_selection();
        if zoom.is_finite() {
            self.zoom = zoom.clamp(0.5, 2.0);
        }
        self.layout = compute_layout(&self.config, self.panel.open);
        if let Some(address) = self.selection.focus {
            self.ensure_visible(address);
        }
        self.clamp_scroll();
    }

    pub fn zoom_in(&mut self) {
        self.set_zoom(self.zoom + 0.1);
    }

    pub fn zoom_out(&mut self) {
        self.set_zoom(self.zoom - 0.1);
    }

    /// Replaces the complete canonical dimension and merge metadata. Existing
    /// sparse row/column size overrides are cleared before the new dimensions
    /// are installed; manual visibility masks remain separate UI projections.
    pub fn apply_sheet_metadata(
        &mut self,
        row_dimensions: &[RowDimension],
        column_dimensions: &[ColumnDimension],
        merges: &[CellRange],
    ) -> UiResult<()> {
        self.update_sheet_metadata(row_dimensions, column_dimensions, merges, true)
    }

    fn update_sheet_metadata(
        &mut self,
        row_dimensions: &[RowDimension],
        column_dimensions: &[ColumnDimension],
        merges: &[CellRange],
        clamp_scroll: bool,
    ) -> UiResult<()> {
        self.validate_sheet_metadata(row_dimensions, column_dimensions, merges)?;
        self.grid.clear_axis_overrides();
        for dimension in row_dimensions {
            self.grid.set_row_size(
                dimension.row,
                dimension_to_pixels(dimension.height, self.config.default_row_height),
            )?;
        }
        for dimension in column_dimensions {
            self.grid.set_column_size(
                dimension.column,
                dimension_to_pixels(dimension.width, self.config.default_column_width),
            )?;
        }
        self.merges = merges.to_vec();
        let range = self.selection_seed.clone().or_else(|| {
            self.selection
                .anchor
                .zip(self.selection.focus)
                .and_then(|(anchor, focus)| CellRange::new(anchor, focus).ok())
        });
        if let Some(range) = range {
            let extent = self.expand_merged_range(range);
            self.selection.kind = Self::kind_for_range(&extent);
            self.selection.extent = Some(extent);
        }
        if clamp_scroll {
            self.clamp_scroll();
        }
        Ok(())
    }

    fn validate_sheet_metadata(
        &self,
        row_dimensions: &[RowDimension],
        column_dimensions: &[ColumnDimension],
        merges: &[CellRange],
    ) -> UiResult<()> {
        for range in merges {
            self.validate_range(range)?;
        }
        for dimension in row_dimensions {
            self.validate_selection_address(CellAddress::new(
                self.active_sheet(),
                dimension.row,
                ColumnIndex(0),
            ))?;
        }
        for dimension in column_dimensions {
            self.validate_selection_address(CellAddress::new(
                self.active_sheet(),
                RowIndex(0),
                dimension.column,
            ))?;
        }
        Ok(())
    }

    /// Applies the union of manual/filter-hidden logical rows supplied by the
    /// canonical sheet owner. This is projection only: no values or criteria
    /// are mutated. Same-sheet sync preserves it; another sheet needs its mask.
    /// A hidden focus prefers the next, then previous visible row in the
    /// selection. It falls outside only if that whole range is hidden, extending
    /// its bounds to include the new focus. No visible rows clears selection.
    pub fn apply_hidden_rows(&mut self, rows: &[RowIndex]) -> UiResult<()> {
        let previous: Vec<_> = self.grid.hidden_rows().collect();
        self.grid.set_hidden_rows(rows)?;
        if self.grid.hidden_rows().eq(previous) {
            return Ok(());
        }
        self.cancel_pointer_selection();
        self.normalize_hidden_focus();
        self.clamp_scroll();
        if let Some(address) = self.selection.focus {
            self.ensure_visible(address);
        }
        self.clamp_scroll();
        Ok(())
    }

    pub fn apply_hidden_columns(&mut self, columns: &[ColumnIndex]) -> UiResult<()> {
        let previous: Vec<_> = self.grid.hidden_columns().collect();
        self.grid.set_hidden_columns(columns)?;
        if self.grid.hidden_columns().eq(previous) {
            return Ok(());
        }
        self.cancel_pointer_selection();
        self.normalize_hidden_focus();
        if let Some(address) = self.selection.focus {
            self.ensure_visible(address);
        }
        self.clamp_scroll();
        Ok(())
    }

    pub fn is_row_hidden(&self, row: RowIndex) -> bool {
        self.grid.is_row_hidden(row)
    }

    pub fn is_column_hidden(&self, column: ColumnIndex) -> bool {
        self.grid.is_column_hidden(column)
    }

    /// Returns the UI-owned frozen split, clamped to current sheet dimensions.
    /// Structural commands must guard operations around a nonzero split: this
    /// projection does not shift freeze counts to follow inserted/deleted axes.
    pub fn frozen_panes(&self) -> FrozenPanes {
        self.config
            .frozen
            .clamp_to_dimensions(&self.config.dimensions)
    }

    fn validate_visible_address(&self, address: CellAddress) -> UiResult<()> {
        self.validate_selection_address(address)?;
        if self.is_row_hidden(address.row) {
            return Err(UiError::InvalidAddress(
                "row is hidden by the sheet visibility mask".into(),
            ));
        }
        if self.is_column_hidden(address.column) {
            return Err(UiError::InvalidAddress(
                "column is hidden by the sheet visibility mask".into(),
            ));
        }
        Ok(())
    }

    fn nearest_visible_row(&self, row: RowIndex) -> Option<RowIndex> {
        self.grid
            .next_visible_row(row)
            .or_else(|| self.grid.previous_visible_row(row))
    }

    fn nearest_visible_column(&self, column: ColumnIndex) -> Option<ColumnIndex> {
        self.grid
            .next_visible_column(column)
            .or_else(|| self.grid.previous_visible_column(column))
    }

    fn normalize_hidden_focus(&mut self) {
        let Some(mut focus) = self.selection.focus else {
            return;
        };
        let hidden_row = self.is_row_hidden(focus.row);
        let hidden_column = self.is_column_hidden(focus.column);
        if !hidden_row && !hidden_column {
            return;
        }
        let old_range = self.selection.range();
        let row = if hidden_row {
            old_range
                .as_ref()
                .and_then(|range| {
                    self.grid
                        .next_visible_row(focus.row)
                        .filter(|row| row.0 <= range.end.row.0)
                        .or_else(|| {
                            self.grid
                                .previous_visible_row(focus.row)
                                .filter(|row| row.0 >= range.start.row.0)
                        })
                })
                .or_else(|| self.nearest_visible_row(focus.row))
        } else {
            Some(focus.row)
        };
        let column = if hidden_column {
            old_range
                .as_ref()
                .and_then(|range| {
                    self.grid
                        .next_visible_column(focus.column)
                        .filter(|column| column.0 <= range.end.column.0)
                        .or_else(|| {
                            self.grid
                                .previous_visible_column(focus.column)
                                .filter(|column| column.0 >= range.start.column.0)
                        })
                })
                .or_else(|| self.nearest_visible_column(focus.column))
        } else {
            Some(focus.column)
        };
        let (Some(row), Some(column)) = (row, column) else {
            self.selection = SelectionState::default();
            self.selection_seed = None;
            self.edit = None;
            self.formula_draft.clear();
            return;
        };
        let seed = self.selection_seed.clone().or_else(|| old_range.clone());
        let collapsed = self.selection.anchor == Some(focus)
            && seed.as_ref().is_some_and(|range| range.start == range.end);
        focus.row = row;
        focus.column = column;
        let anchor = if collapsed {
            focus
        } else {
            self.selection.anchor.unwrap_or(focus)
        };
        let mut range = if collapsed {
            CellRange::new(focus, focus).expect("same sheet")
        } else {
            seed.unwrap_or_else(|| CellRange::new(anchor, focus).expect("same sheet"))
        };
        range.start.row.0 = range.start.row.0.min(focus.row.0);
        range.end.row.0 = range.end.row.0.max(focus.row.0);
        range.start.column.0 = range.start.column.0.min(focus.column.0);
        range.end.column.0 = range.end.column.0.max(focus.column.0);
        let extent = self.expand_merged_range(range.clone());
        self.selection = SelectionState {
            kind: Self::kind_for_range(&extent),
            anchor: Some(anchor),
            focus: Some(focus),
            extent: Some(extent),
        };
        self.selection_seed = Some(range);
        self.edit = None;
        self.formula_draft.clear();
    }

    fn clamp_scroll(&mut self) {
        // Floor the physical inverse viewport for integer scroll bounds; the
        // frame/hit viewport uses ceil so its last fractional strip is visible.
        let viewport = Viewport::new(
            0.0,
            0.0,
            (self.layout.grid_viewport.w / f64::from(self.zoom)).max(1.0),
            (self.layout.grid_viewport.h / f64::from(self.zoom)).max(1.0),
        );
        self.scroll = self.grid.clamp_scroll(viewport, self.scroll);
    }

    pub fn select_cell(&mut self, address: CellAddress) -> UiResult<()> {
        self.validate_visible_address(address)?;
        self.cancel_pointer_selection();
        self.set_selection_endpoints(address, address);
        Ok(())
    }

    pub fn select_range(&mut self, start: CellAddress, end: CellAddress) -> UiResult<()> {
        let range = CellRange::new(start, end)
            .map_err(|error| UiError::InvalidAddress(error.to_string()))?;
        if range.sheet != self.snapshot.active_sheet {
            return Err(UiError::InvalidAddress(
                "range must belong to the active sheet".into(),
            ));
        }
        self.config.dimensions.validate_address(range.start)?;
        self.config.dimensions.validate_address(range.end)?;
        self.validate_visible_address(range.start)?;
        self.validate_visible_address(range.end)?;

        self.cancel_pointer_selection();
        self.set_selection_endpoints(range.start, range.end);
        // Go To retains RC24's normalized endpoints and shows the range start.
        self.ensure_visible(range.start);
        Ok(())
    }

    /// Ends a gesture without changing selection. Required on capture loss or
    /// app-level document switches; a subsequent stale PointerUp is ignored.
    pub fn cancel_pointer_selection(&mut self) {
        self.dragging = false;
    }

    /// Restores a snapshot only after validating every address and range. A
    /// rejected snapshot leaves selection, scroll, focus and gesture untouched.
    /// Snapshots omit merge provenance: endpoints seed later metadata updates
    /// when their merge closure reproduces the extent; otherwise explicit
    /// snapshot bounds are retained. Hidden focus is normalized after validation.
    pub fn restore_selection(&mut self, selection: SelectionState) -> UiResult<()> {
        if selection.kind == SelectionKind::None {
            if selection.anchor.is_some() || selection.focus.is_some() || selection.extent.is_some()
            {
                return Err(UiError::InvalidState("empty selection has addresses"));
            }
            self.cancel_pointer_selection();
            self.selection = selection;
            self.selection_seed = None;
            return Ok(());
        }
        let anchor = selection
            .anchor
            .ok_or(UiError::InvalidState("missing selection anchor"))?;
        let focus = selection
            .focus
            .ok_or(UiError::InvalidState("missing selection focus"))?;
        self.validate_selection_address(anchor)?;
        self.validate_selection_address(focus)?;
        let range = match selection.extent {
            Some(range) => {
                self.validate_range(&range)?;
                if !range.contains(&anchor) || !range.contains(&focus) {
                    return Err(UiError::InvalidState("selection extent excludes endpoints"));
                }
                range
            }
            None => CellRange::new(anchor, focus)
                .map_err(|error| UiError::InvalidAddress(error.to_string()))?,
        };
        let extent = self.expand_merged_range(range.clone());
        let endpoints = CellRange::new(anchor, focus).expect("validated same-sheet endpoints");
        // A snapshot has no merge provenance. Prefer directional endpoints
        // when they reproduce its extent; otherwise retain explicit bounds.
        self.selection_seed = Some(if self.expand_merged_range(endpoints.clone()) == extent {
            endpoints
        } else {
            range
        });
        self.cancel_pointer_selection();
        self.selection = SelectionState {
            kind: Self::kind_for_range(&extent),
            anchor: Some(anchor),
            focus: Some(focus),
            extent: Some(extent),
        };
        self.focus = FocusTarget::Grid;
        self.normalize_hidden_focus();
        if let Some(address) = self.selection.focus {
            self.ensure_visible(address);
        }
        Ok(())
    }

    fn validate_selection_address(&self, address: CellAddress) -> UiResult<()> {
        if address.sheet != self.active_sheet() {
            return Err(UiError::InvalidAddress(
                "cell belongs to a different active sheet".into(),
            ));
        }
        self.config.dimensions.validate_address(address)?;
        Ok(())
    }

    fn validate_range(&self, range: &CellRange) -> UiResult<()> {
        range
            .validate()
            .map_err(|error| UiError::InvalidAddress(error.to_string()))?;
        self.validate_selection_address(range.start)?;
        self.validate_selection_address(range.end)
    }

    fn kind_for_range(range: &CellRange) -> SelectionKind {
        if range.start == range.end {
            SelectionKind::Cell
        } else {
            SelectionKind::Range
        }
    }

    fn set_selection_endpoints(&mut self, anchor: CellAddress, focus: CellAddress) {
        // Both endpoints are validated by public boundaries or grid hit testing.
        let range = CellRange::new(anchor, focus).expect("selection endpoints share a sheet");
        let extent = self.expand_merged_range(range.clone());
        self.selection = SelectionState {
            kind: Self::kind_for_range(&extent),
            anchor: Some(anchor),
            focus: Some(focus),
            extent: Some(extent),
        };
        self.selection_seed = Some(range);
        self.focus = FocusTarget::Grid;
        self.normalize_hidden_focus();
        if let Some(address) = self.selection.focus {
            self.ensure_visible(address);
        }
    }

    fn expand_merged_range(&self, mut range: CellRange) -> CellRange {
        // Only scan existing metadata, never cells in the selected rectangle.
        // Every changing pass includes a previously excluded merge, so at most
        // merges.len() changing passes are possible, including chained merges.
        for _ in 0..=self.merges.len() {
            let before = range.clone();
            for merged in &self.merges {
                if range.intersects(merged) {
                    range.start.row.0 = range.start.row.0.min(merged.start.row.0);
                    range.start.column.0 = range.start.column.0.min(merged.start.column.0);
                    range.end.row.0 = range.end.row.0.max(merged.end.row.0);
                    range.end.column.0 = range.end.column.0.max(merged.end.column.0);
                }
            }
            if range == before {
                break;
            }
        }
        range
    }

    fn handle_pointer_down(
        &mut self,
        point: Point,
        button: PointerButton,
        modifiers: Modifiers,
    ) -> Vec<UiEffect> {
        if button != PointerButton::Primary {
            return Vec::new();
        }
        self.cancel_pointer_selection();
        if let Some(address) = self.hit_test_grid(point) {
            let anchor = if modifiers.shift {
                self.selection.anchor.unwrap_or(address)
            } else {
                address
            };
            self.set_selection_endpoints(anchor, address);
            self.edit = None;
            self.formula_draft.clear();
            self.dragging = true;
        }
        Vec::new()
    }
    fn handle_pointer_move(&mut self, point: Point, _modifiers: Modifiers) -> Vec<UiEffect> {
        if self.dragging {
            if let Some(address) = self.hit_test_grid(point) {
                if let Some(anchor) = self.selection.anchor {
                    self.set_selection_endpoints(anchor, address);
                }
            } else {
                self.cancel_pointer_selection();
            }
        }
        Vec::new()
    }
    fn handle_pointer_up(
        &mut self,
        point: Point,
        button: PointerButton,
        _modifiers: Modifiers,
    ) -> Vec<UiEffect> {
        if button == PointerButton::Primary && self.dragging {
            self.handle_pointer_move(point, Modifiers::default());
            self.cancel_pointer_selection();
        }
        Vec::new()
    }
    fn handle_double_click(&mut self, point: Point) -> Vec<UiEffect> {
        self.cancel_pointer_selection();
        if let Some(address) = self.hit_test_grid(point) {
            self.set_selection_endpoints(address, address);
            let address = self.merged_anchor(address);
            self.edit = Some(EditSession {
                address,
                original: String::new(),
                draft: String::new(),
                source: EditSource::Grid,
            });
        }
        Vec::new()
    }
    fn handle_wheel(&mut self, delta_x: i64, delta_y: i64) -> Vec<UiEffect> {
        self.scroll.x = if delta_x.is_negative() {
            self.scroll.x.saturating_sub(delta_x.unsigned_abs() as u32)
        } else {
            self.scroll.x.saturating_add(delta_x as u32)
        };
        self.scroll.y = if delta_y.is_negative() {
            self.scroll.y.saturating_sub(delta_y.unsigned_abs() as u32)
        } else {
            self.scroll.y.saturating_add(delta_y as u32)
        };
        self.layout = compute_layout(&self.config, self.panel.open);
        self.clamp_scroll();
        Vec::new()
    }
    fn handle_key(&mut self, key: Key, modifiers: Modifiers) -> Vec<UiEffect> {
        if self.focus != FocusTarget::Grid {
            return Vec::new();
        }
        if self.edit.is_some() {
            return match key {
                Key::Escape => {
                    self.cancel_pointer_selection();
                    self.edit = None;
                    self.formula_draft.clear();
                    Vec::new()
                }
                Key::Enter if self.edit.is_some() => self.commit_active_cell(),
                _ => Vec::new(),
            };
        }
        self.cancel_pointer_selection();
        match key {
            Key::ArrowUp => self.move_selection(-1, 0, modifiers.shift),
            Key::ArrowDown => self.move_selection(1, 0, modifiers.shift),
            Key::ArrowLeft => self.move_selection(0, -1, modifiers.shift),
            Key::ArrowRight => self.move_selection(0, 1, modifiers.shift),
            Key::Character('a' | 'A') if modifiers.control && !modifiers.alt && !modifiers.meta => {
                self.select_known_used_region();
                Vec::new()
            }
            Key::Escape => {
                self.edit = None;
                self.formula_draft.clear();
                Vec::new()
            }
            Key::Enter => {
                if self.edit.is_some() {
                    self.commit_active_cell()
                } else {
                    self.move_selection(1, 0, false)
                }
            }
            Key::Tab => self.move_selection(0, 1, false),
            Key::F2 => self.start_formula_edit(),
            _ => Vec::new(),
        }
    }
    fn handle_text_input(&mut self, text: String) -> Vec<UiEffect> {
        if let Some(edit) = &mut self.edit {
            edit.draft.push_str(&text);
            self.formula_draft = edit.draft.clone();
        }
        Vec::new()
    }
    fn commit_formula_bar(&mut self) -> Vec<UiEffect> {
        if let Some(address) = self.active_input_address() {
            let raw = std::mem::take(&mut self.formula_draft);
            return vec![UiEffect::CommitCellRaw { address, raw }];
        }
        Vec::new()
    }
    fn commit_name_box(&mut self, _text: String) -> Vec<UiEffect> {
        Vec::new()
    }
    fn activate_sheet(&mut self, sheet: SheetId) -> Vec<UiEffect> {
        self.cancel_pointer_selection();
        vec![UiEffect::ActivateSheet(sheet)]
    }
    fn select_known_used_region(&mut self) {
        if self.config.dimensions.rows == 0 || self.config.dimensions.columns == 0 {
            return;
        }
        let mut bounds: Option<CellRange> = None;
        for (start, end) in self
            .snapshot
            .cells
            .iter()
            .map(|cell| (cell.address, cell.address))
            .chain(self.merges.iter().map(|range| (range.start, range.end)))
        {
            bounds = Some(match bounds {
                Some(mut range) => {
                    range.start.row.0 = range.start.row.0.min(start.row.0);
                    range.start.column.0 = range.start.column.0.min(start.column.0);
                    range.end.row.0 = range.end.row.0.max(end.row.0);
                    range.end.column.0 = range.end.column.0.max(end.column.0);
                    range
                }
                None => CellRange::new(start, end).expect("snapshot addresses share a sheet"),
            });
        }
        let range = bounds.unwrap_or_else(|| {
            let origin = CellAddress::new(self.active_sheet(), RowIndex(0), ColumnIndex(0));
            CellRange::new(origin, origin).expect("same origin")
        });
        self.set_selection_endpoints(range.start, range.end);
    }

    fn move_selection(&mut self, delta_row: i32, delta_col: i32, extend: bool) -> Vec<UiEffect> {
        if self.config.dimensions.rows == 0 || self.config.dimensions.columns == 0 {
            return Vec::new();
        }
        let mut current = self.selection.focus.unwrap_or(CellAddress::new(
            self.snapshot.active_sheet,
            RowIndex(0),
            ColumnIndex(0),
        ));
        let Some(visible) = self.nearest_visible_row(current.row) else {
            return Vec::new();
        };
        current.row = visible;
        let row = if delta_row > 0 {
            current
                .row
                .0
                .checked_add(delta_row as u32)
                .and_then(|row| self.grid.next_visible_row(RowIndex(row)))
        } else if delta_row < 0 {
            current
                .row
                .0
                .checked_sub(delta_row.unsigned_abs())
                .and_then(|row| self.grid.previous_visible_row(RowIndex(row)))
        } else {
            Some(current.row)
        }
        .unwrap_or(current.row);
        let Some(column) = self.nearest_visible_column(current.column) else {
            return Vec::new();
        };
        current.column = column;
        let column = if delta_col > 0 {
            current
                .column
                .0
                .checked_add(delta_col as u32)
                .and_then(|column| self.grid.next_visible_column(ColumnIndex(column)))
        } else if delta_col < 0 {
            current
                .column
                .0
                .checked_sub(delta_col.unsigned_abs())
                .and_then(|column| self.grid.previous_visible_column(ColumnIndex(column)))
        } else {
            Some(current.column)
        }
        .unwrap_or(current.column);
        let address = CellAddress::new(current.sheet, row, column);
        let anchor = if extend {
            self.selection.anchor.unwrap_or(current)
        } else {
            address
        };
        self.set_selection_endpoints(anchor, address);
        self.layout = compute_layout(&self.config, self.panel.open);
        Vec::new()
    }
    fn commit_active_cell(&mut self) -> Vec<UiEffect> {
        if let Some(address) = self
            .edit
            .as_ref()
            .map(|edit| edit.address)
            .or_else(|| self.active_input_address())
        {
            let raw = self
                .edit
                .as_ref()
                .map(|e| e.draft.clone())
                .unwrap_or_default();
            if !raw.is_empty() {
                return vec![UiEffect::CommitCellRaw { address, raw }];
            }
        }
        Vec::new()
    }
    fn start_formula_edit(&mut self) -> Vec<UiEffect> {
        if let Some(address) = self.active_input_address() {
            self.edit = Some(EditSession {
                address,
                original: String::new(),
                draft: String::new(),
                source: EditSource::Keyboard,
            });
            self.formula_draft = String::new();
        }
        Vec::new()
    }
    fn apply_format(&mut self, _patch: FormatPatch) -> Vec<UiEffect> {
        if let Some(range) = self.selection.range() {
            return vec![UiEffect::ApplyFormat {
                range,
                patch: _patch,
            }];
        }
        Vec::new()
    }
    fn ensure_visible(&mut self, address: CellAddress) {
        let Some(rect) = self.grid.cell_rect(address) else {
            return;
        };
        let view_w = (self.layout.grid_viewport.w / f64::from(self.zoom)).max(1.0);
        let view_h = (self.layout.grid_viewport.h / f64::from(self.zoom)).max(1.0);
        let frozen = self
            .config
            .frozen
            .clamp_to_dimensions(&self.config.dimensions);
        let (frozen_width, frozen_height) = self.frozen_extent();
        if address.column.0 >= frozen.cols {
            if rect.x < self.scroll.x as f64 + frozen_width {
                self.scroll.x = (rect.x - frozen_width).max(0.0) as u32;
            } else if rect.right() > self.scroll.x as f64 + view_w {
                self.scroll.x = (rect.right() - view_w).max(0.0).ceil() as u32;
            }
        }
        if address.row.0 >= frozen.rows {
            if rect.y < self.scroll.y as f64 + frozen_height {
                self.scroll.y = (rect.y - frozen_height).max(0.0) as u32;
            } else if rect.bottom() > self.scroll.y as f64 + view_h {
                self.scroll.y = (rect.bottom() - view_h).max(0.0).ceil() as u32;
            }
        }
    }

    fn merged_anchor(&self, address: CellAddress) -> CellAddress {
        self.merges
            .iter()
            .find(|range| range.contains(&address))
            .map(|range| range.start)
            .unwrap_or(address)
    }

    fn frozen_extent(&self) -> (f64, f64) {
        let frozen = self
            .config
            .frozen
            .clamp_to_dimensions(&self.config.dimensions);
        let width = frozen
            .cols
            .checked_sub(1)
            .and_then(|col| {
                self.grid.cell_rect(CellAddress::new(
                    self.active_sheet(),
                    RowIndex(0),
                    ColumnIndex(col),
                ))
            })
            .map(|rect| rect.right())
            .unwrap_or(0.0);
        let height = frozen
            .rows
            .checked_sub(1)
            .and_then(|row| {
                self.grid.cell_rect(CellAddress::new(
                    self.active_sheet(),
                    RowIndex(row),
                    ColumnIndex(0),
                ))
            })
            .map(|rect| rect.bottom())
            .unwrap_or(0.0);
        (width, height)
    }

    fn cell_pane(&self, address: CellAddress) -> Rect {
        let viewport = self.layout.grid_viewport;
        let frozen = self
            .config
            .frozen
            .clamp_to_dimensions(&self.config.dimensions);
        let (width, height) = self.frozen_extent();
        let freeze_x = (viewport.x + width * f64::from(self.zoom)).min(viewport.right());
        let freeze_y = (viewport.y + height * f64::from(self.zoom)).min(viewport.bottom());
        let (left, right) = if address.column.0 < frozen.cols {
            (viewport.x, freeze_x)
        } else {
            (freeze_x, viewport.right())
        };
        let (top, bottom) = if address.row.0 < frozen.rows {
            (viewport.y, freeze_y)
        } else {
            (freeze_y, viewport.bottom())
        };
        Rect::new(left, top, right - left, bottom - top)
    }

    fn hit_test_grid(&self, point: Point) -> Option<CellAddress> {
        // Points are physical pixels local to the complete UiFrame canvas,
        // including its headers/formula bar. Adapters must NOT pre-apply zoom.
        let viewport = self.layout.grid_viewport;
        if !viewport.contains(point) {
            return None;
        }
        let zoom = f64::from(self.zoom);
        self.grid.hit_test(
            Point::new((point.x - viewport.x) / zoom, (point.y - viewport.y) / zoom),
            self.logical_grid_viewport(),
            self.scroll,
        )
    }
}

fn dimension_to_pixels(value: f64, fallback: u32) -> u32 {
    if value.is_finite() && value > 0.0 {
        value.round().clamp(8.0, 4096.0) as u32
    } else {
        fallback
    }
}

fn zoom_rect(rect: Rect, origin: Rect, zoom: f64) -> Rect {
    Rect::new(
        origin.x + (rect.x - origin.x) * zoom,
        origin.y + (rect.y - origin.y) * zoom,
        rect.w * zoom,
        rect.h * zoom,
    )
}

fn compute_layout(config: &UiConfig, panel_open: bool) -> UiLayout {
    let w = config.outer_width as f64;
    let h = config.outer_height as f64;
    let status_h = config.metrics.status_bar_height as f64;
    let tabs_h = config.metrics.sheet_tabs_height as f64;
    let formula_h = config.metrics.formula_bar_height as f64;
    let row_h = config.metrics.row_header_width as f64;
    let panel_w = config.metrics.formatting_panel_width as f64;
    let scroll_w = config.metrics.scrollbar_size as f64;
    let header_h = config.metrics.column_header_height as f64;
    let panel_w_actual = if panel_open
        && w >= panel_w + row_h + config.default_column_width as f64 + scroll_w * 2.0
    {
        panel_w
    } else {
        0.0
    };
    let formula_w = (w - config.metrics.name_box_width as f64).max(0.0);
    let viewport_w = (w - row_h - scroll_w - panel_w_actual).max(0.0);
    let viewport_h = (h - formula_h - header_h - status_h - tabs_h).max(0.0);
    UiLayout {
        name_box: Rect::new(0.0, 0.0, config.metrics.name_box_width as f64, formula_h),
        formula_bar: Rect::new(
            config.metrics.name_box_width as f64,
            0.0,
            formula_w,
            formula_h,
        ),
        corner: Rect::new(0.0, formula_h, row_h, header_h),
        column_header: Rect::new(row_h, formula_h, viewport_w, header_h),
        row_header: Rect::new(0.0, formula_h + header_h, row_h, viewport_h),
        grid_viewport: Rect::new(row_h, formula_h + header_h, viewport_w, viewport_h),
        horizontal_scrollbar: Rect::new(
            row_h,
            formula_h + header_h + viewport_h,
            viewport_w,
            scroll_w,
        ),
        vertical_scrollbar: Rect::new(
            row_h + viewport_w,
            formula_h + header_h,
            scroll_w,
            viewport_h,
        ),
        sheet_tabs: Rect::new(0.0, (h - tabs_h).max(0.0), w, tabs_h),
        status_bar: Rect::new(0.0, (h - tabs_h - status_h).max(0.0), w, status_h),
        formatting_panel: if panel_open
            && w >= panel_w + row_h + config.default_column_width as f64 + scroll_w * 2.0
        {
            Some(Rect::new((w - panel_w).max(0.0), 0.0, panel_w, h))
        } else {
            None
        },
    }
}

pub fn parse_a1_range(
    input: &str,
    sheet: SheetId,
    dimensions: GridDimensions,
) -> UiResult<(CellAddress, CellAddress)> {
    if dimensions.rows == 0 || dimensions.columns == 0 {
        return Err(UiError::InvalidInput(
            "range dimensions must be non-zero".into(),
        ));
    }

    let (start_input, end_input) = match input.split_once(':') {
        Some((start, end)) if !end.contains(':') => (start, end),
        Some(_) => {
            return Err(UiError::InvalidInput(
                "range must contain at most one colon".into(),
            ));
        }
        None => (input, input),
    };
    let start = parse_a1_cell(start_input, sheet)?;
    let end = parse_a1_cell(end_input, sheet)?;
    let canonical_start = CellAddress::new(
        sheet,
        RowIndex(start.row.0.min(end.row.0)),
        ColumnIndex(start.column.0.min(end.column.0)),
    );
    let canonical_end = CellAddress::new(
        sheet,
        RowIndex(start.row.0.max(end.row.0)),
        ColumnIndex(start.column.0.max(end.column.0)),
    );
    dimensions.validate_address(canonical_start)?;
    dimensions.validate_address(canonical_end)?;
    Ok((canonical_start, canonical_end))
}

fn parse_a1_cell(input: &str, sheet: SheetId) -> UiResult<CellAddress> {
    let bytes = input.as_bytes();
    let mut index = 0;
    if bytes.get(index) == Some(&b'$') {
        index += 1;
    }
    let column_start = index;
    let mut column_value = 0u32;
    while let Some(&byte) = bytes.get(index) {
        if !byte.is_ascii_alphabetic() {
            break;
        }
        let letter = byte.to_ascii_uppercase() - b'A' + 1;
        column_value = column_value
            .checked_mul(26)
            .and_then(|value| value.checked_add(u32::from(letter)))
            .ok_or_else(|| UiError::InvalidInput("column is too large".into()))?;
        index += 1;
    }
    if index == column_start {
        return Err(UiError::InvalidInput("missing column".into()));
    }
    if bytes.get(index) == Some(&b'$') {
        index += 1;
    }
    let row_start = index;
    let mut row_value = 0u32;
    while let Some(&byte) = bytes.get(index) {
        if !byte.is_ascii_digit() {
            return Err(UiError::InvalidInput("malformed cell reference".into()));
        }
        row_value = row_value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u32::from(byte - b'0')))
            .ok_or_else(|| UiError::InvalidInput("row is too large".into()))?;
        index += 1;
    }
    if index == row_start || row_value == 0 {
        return Err(UiError::InvalidInput("row must be 1-based".into()));
    }

    let column = column_value
        .checked_sub(1)
        .ok_or_else(|| UiError::InvalidInput("missing column".into()))?;
    let row = row_value
        .checked_sub(1)
        .ok_or_else(|| UiError::InvalidInput("row must be 1-based".into()))?;
    Ok(CellAddress::new(sheet, RowIndex(row), ColumnIndex(column)))
}

fn col_to_label(mut col: u32) -> String {
    let mut label = String::new();
    loop {
        label.push((b'A' + (col % 26) as u8) as char);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    label.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(row: u32, column: u32) -> CellAddress {
        CellAddress::new(SheetId(0), RowIndex(row), ColumnIndex(column))
    }

    fn point_for_cell(ui: &SheetsUi, address: CellAddress) -> Point {
        let frame = ui.frame().unwrap();
        let rect = frame
            .cells
            .iter()
            .find(|cell| cell.address == address)
            .expect("cell is in the bounded frame")
            .visible_rect
            .expect("cell is visible");
        Point::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
    }

    fn pointer_down(ui: &mut SheetsUi, address: CellAddress, shift: bool) {
        let point = point_for_cell(ui, address);
        assert!(
            ui.handle_event(UiEvent::PointerDown {
                point,
                button: PointerButton::Primary,
                modifiers: Modifiers {
                    shift,
                    ..Modifiers::default()
                },
            })
            .unwrap()
            .is_empty()
        );
    }

    fn pointer_move(ui: &mut SheetsUi, address: CellAddress) {
        let point = point_for_cell(ui, address);
        assert!(
            ui.handle_event(UiEvent::PointerMove {
                point,
                modifiers: Modifiers::default(),
            })
            .unwrap()
            .is_empty()
        );
    }

    fn pointer_up(ui: &mut SheetsUi, address: CellAddress) {
        let point = point_for_cell(ui, address);
        assert!(
            ui.handle_event(UiEvent::PointerUp {
                point,
                button: PointerButton::Primary,
                modifiers: Modifiers::default(),
            })
            .unwrap()
            .is_empty()
        );
    }

    fn shift_key(ui: &mut SheetsUi, key: Key) {
        assert!(
            ui.handle_event(UiEvent::KeyDown {
                key,
                modifiers: Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
            })
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn mouse_range_matrix_preserves_direction_and_normalizes_bounds() {
        for (start, end) in [
            (cell(0, 0), cell(2, 2)),
            (cell(2, 2), cell(0, 0)),
            (cell(2, 0), cell(0, 2)),
            (cell(0, 2), cell(2, 0)),
            (cell(1, 1), cell(1, 1)),
        ] {
            let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
            pointer_down(&mut ui, start, false);
            pointer_move(&mut ui, end);
            pointer_up(&mut ui, end);
            assert_eq!(ui.selection.anchor, Some(start));
            assert_eq!(ui.selection.focus, Some(end));
            assert_eq!(
                ui.selection.range(),
                Some(CellRange::new(start, end).unwrap())
            );
            assert!(!ui.dragging);
            let frame = ui.frame().unwrap();
            let range = ui.selection.range().unwrap();
            assert_eq!(
                frame.cells.iter().filter(|cell| cell.selected).count() as u64,
                range.rows() * range.cols()
            );
            assert_eq!(frame.cells.iter().filter(|cell| cell.active).count(), 1);
        }
    }

    #[test]
    fn mouse_drag_reverses_through_anchor_and_release_updates_last_extent() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        let anchor = cell(2, 2);
        pointer_down(&mut ui, anchor, false);
        pointer_move(&mut ui, cell(0, 0));
        pointer_move(&mut ui, cell(3, 3));
        pointer_up(&mut ui, cell(1, 3));
        assert_eq!(ui.selection.anchor, Some(anchor));
        assert_eq!(ui.selection.focus, Some(cell(1, 3)));
        assert_eq!(
            ui.selection.range(),
            Some(CellRange::new(cell(1, 2), cell(2, 3)).unwrap())
        );
        pointer_move(&mut ui, cell(0, 0));
        assert_eq!(ui.selection.focus, Some(cell(1, 3)));
    }

    #[test]
    fn shift_click_and_all_arrows_expand_contract_and_cross_anchor() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        let anchor = cell(1, 1);
        ui.select_cell(anchor).unwrap();
        pointer_down(&mut ui, cell(2, 3), true);
        pointer_up(&mut ui, cell(2, 3));
        assert_eq!(ui.selection.anchor, Some(anchor));
        assert_eq!(
            ui.selection.range(),
            Some(CellRange::new(anchor, cell(2, 3)).unwrap())
        );
        ui.select_cell(anchor).unwrap();
        for (key, active) in [
            (Key::ArrowRight, cell(1, 2)),
            (Key::ArrowLeft, anchor),
            (Key::ArrowLeft, cell(1, 0)),
            (Key::ArrowDown, cell(2, 0)),
            (Key::ArrowUp, cell(1, 0)),
            (Key::ArrowUp, cell(0, 0)),
            (Key::ArrowRight, cell(0, 1)),
            (Key::ArrowDown, anchor),
        ] {
            shift_key(&mut ui, key);
            assert_eq!(ui.selection.anchor, Some(anchor));
            assert_eq!(ui.selection.focus, Some(active));
            assert_eq!(
                ui.selection.range(),
                Some(CellRange::new(anchor, active).unwrap())
            );
        }
        assert_eq!(ui.selection.kind, SelectionKind::Cell);
        ui.handle_event(UiEvent::KeyDown {
            key: Key::ArrowRight,
            modifiers: Modifiers::default(),
        })
        .unwrap();
        assert_eq!(ui.selection.anchor, Some(cell(1, 2)));
    }

    #[test]
    fn merged_click_and_chained_drag_expand_extent_without_moving_endpoints() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        let merge = CellRange::new(cell(0, 2), cell(1, 3)).unwrap();
        ui.apply_sheet_metadata(&[], &[], std::slice::from_ref(&merge))
            .unwrap();
        pointer_down(&mut ui, cell(1, 3), false);
        pointer_up(&mut ui, cell(1, 3));
        assert_eq!(ui.selection.anchor, Some(cell(1, 3)));
        assert_eq!(ui.selection.focus, Some(cell(1, 3)));
        assert_eq!(ui.selection.range(), Some(merge.clone()));
        let merges = [
            CellRange::new(cell(2, 3), cell(3, 4)).unwrap(),
            CellRange::new(cell(1, 0), cell(2, 1)).unwrap(),
            merge,
        ];
        ui.apply_sheet_metadata(&[], &[], &merges).unwrap();
        pointer_down(&mut ui, cell(0, 0), false);
        pointer_move(&mut ui, cell(0, 2));
        assert_eq!(ui.selection.anchor, Some(cell(0, 0)));
        assert_eq!(ui.selection.focus, Some(cell(0, 2)));
        assert_eq!(
            ui.selection.range(),
            Some(CellRange::new(cell(0, 0), cell(3, 4)).unwrap())
        );
        pointer_move(&mut ui, cell(0, 1));
        assert_eq!(
            ui.selection.range(),
            Some(CellRange::new(cell(0, 0), cell(0, 1)).unwrap())
        );
        shift_key(&mut ui, Key::ArrowRight);
        assert_eq!(
            ui.selection.range(),
            Some(CellRange::new(cell(0, 0), cell(3, 4)).unwrap())
        );
        shift_key(&mut ui, Key::ArrowLeft);
        assert_eq!(
            ui.selection.range(),
            Some(CellRange::new(cell(0, 0), cell(0, 1)).unwrap())
        );
    }

    #[test]
    fn pointer_hit_testing_matches_scrolled_zoomed_variable_frame_geometry() {
        for zoom in [0.5, 1.0, 2.0] {
            let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
            ui.apply_sheet_metadata(
                &[
                    RowDimension {
                        row: RowIndex(0),
                        height: 40.0,
                    },
                    RowDimension {
                        row: RowIndex(1),
                        height: 80.0,
                    },
                ],
                &[
                    ColumnDimension {
                        column: ColumnIndex(0),
                        width: 80.0,
                    },
                    ColumnDimension {
                        column: ColumnIndex(1),
                        width: 120.0,
                    },
                ],
                &[],
            )
            .unwrap();
            ui.set_zoom(zoom);
            ui.scroll = ScrollOffset { x: 50, y: 20 };
            pointer_down(&mut ui, cell(1, 1), false);
            pointer_move(&mut ui, cell(2, 2));
            pointer_up(&mut ui, cell(2, 2));
            assert_eq!(ui.selection.anchor, Some(cell(1, 1)));
            assert_eq!(ui.selection.focus, Some(cell(2, 2)));
        }
    }

    #[test]
    fn frozen_cell_hit_testing_and_visibility_preserve_body_scroll() {
        let mut ui = SheetsUi::new(UiConfig {
            frozen: FrozenPanes::with(1, 1),
            ..UiConfig::default()
        })
        .unwrap();
        ui.set_zoom(2.0);
        ui.scroll = ScrollOffset { x: 500, y: 200 };
        let before = ui.scroll;
        pointer_down(&mut ui, cell(0, 0), false);
        pointer_up(&mut ui, cell(0, 0));
        assert_eq!(ui.selection.focus, Some(cell(0, 0)));
        assert_eq!(ui.scroll, before);
        ui.select_cell(cell(10, 10)).unwrap();
        let point = point_for_cell(&ui, cell(10, 10));
        assert_eq!(ui.hit_test_grid(point), Some(cell(10, 10)));
    }

    #[test]
    fn shift_navigation_scrolls_active_cell_into_bounded_visible_frame() {
        let mut ui = SheetsUi::new(UiConfig {
            outer_width: 320,
            outer_height: 230,
            ..UiConfig::default()
        })
        .unwrap();
        ui.set_zoom(2.0);
        ui.select_cell(cell(0, 0)).unwrap();
        for _ in 0..20 {
            shift_key(&mut ui, Key::ArrowDown);
            shift_key(&mut ui, Key::ArrowRight);
        }
        assert_eq!(ui.selection.anchor, Some(cell(0, 0)));
        assert_eq!(ui.selection.focus, Some(cell(20, 20)));
        let frame = ui.frame().unwrap();
        let active = frame.cells.iter().find(|cell| cell.active).unwrap();
        assert!(active.rect.x >= frame.layout.grid_viewport.x);
        assert!(active.rect.y >= frame.layout.grid_viewport.y);
        assert!(active.rect.right() <= frame.layout.grid_viewport.right());
        assert!(active.rect.bottom() <= frame.layout.grid_viewport.bottom());
        assert!(frame.cells.len() < 200);
    }

    #[test]
    fn cancellation_and_pointer_boundaries_prevent_stale_release_selection() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        for action in 0..6 {
            pointer_down(&mut ui, cell(0, 0), false);
            pointer_move(&mut ui, cell(1, 1));
            match action {
                0 => ui.cancel_pointer_selection(),
                1 => {
                    ui.handle_event(UiEvent::PointerLeave).unwrap();
                }
                2 => {
                    ui.handle_event(UiEvent::Resize {
                        width: 900,
                        height: 700,
                    })
                    .unwrap();
                }
                3 => {
                    ui.handle_event(UiEvent::ActivateSheet(SheetId(1))).unwrap();
                }
                4 => ui.set_zoom(0.5),
                _ => {
                    ui.handle_event(UiEvent::PointerMove {
                        point: Point::new(0.0, 0.0),
                        modifiers: Modifiers::default(),
                    })
                    .unwrap();
                }
            }
            let saved = ui.selection.clone();
            assert!(!ui.dragging);
            pointer_up(&mut ui, cell(2, 2));
            pointer_move(&mut ui, cell(2, 2));
            assert_eq!(ui.selection, saved);
        }
        let saved = ui.selection.clone();
        ui.handle_event(UiEvent::PointerDown {
            point: point_for_cell(&ui, cell(3, 3)),
            button: PointerButton::Secondary,
            modifiers: Modifiers::default(),
        })
        .unwrap();
        assert!(!ui.dragging);
        assert_eq!(ui.selection, saved);
        ui.handle_event(UiEvent::PointerDown {
            point: Point::new(10.0, 10.0),
            button: PointerButton::Primary,
            modifiers: Modifiers::default(),
        })
        .unwrap();
        assert!(!ui.dragging);
        assert_eq!(ui.selection, saved);
    }

    #[test]
    fn scrolling_during_drag_uses_new_logical_hit_without_losing_anchor() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        pointer_down(&mut ui, cell(0, 0), false);
        ui.handle_event(UiEvent::Wheel {
            delta_x: 200,
            delta_y: 120,
        })
        .unwrap();
        pointer_move(&mut ui, cell(5, 4));
        pointer_up(&mut ui, cell(5, 4));
        assert_eq!(ui.selection.anchor, Some(cell(0, 0)));
        assert_eq!(ui.selection.focus, Some(cell(5, 4)));
    }

    #[test]
    fn restore_rejects_invalid_snapshots_without_any_partial_state_change() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        pointer_down(&mut ui, cell(1, 1), false);
        pointer_move(&mut ui, cell(2, 2));
        let saved = ui.selection.clone();
        let scroll = ui.scroll;
        let focus_target = ui.focus;
        let mut wrong_sheet = saved.clone();
        wrong_sheet.focus.as_mut().unwrap().sheet = SheetId(1);
        let mut outside = saved.clone();
        outside.anchor.as_mut().unwrap().row = RowIndex(ui.config.dimensions.rows);
        let mut missing = saved.clone();
        missing.anchor = None;
        let mut extent_outside = saved.clone();
        extent_outside.extent.as_mut().unwrap().end.column =
            ColumnIndex(ui.config.dimensions.columns);
        let mut malformed = saved.clone();
        malformed.extent.as_mut().unwrap().sheet = SheetId(1);
        let mut excluding = saved.clone();
        excluding.extent = Some(CellRange::new(cell(0, 0), cell(0, 0)).unwrap());
        for invalid in [
            wrong_sheet,
            outside,
            missing,
            extent_outside,
            malformed,
            excluding,
            SelectionState {
                kind: SelectionKind::None,
                ..saved.clone()
            },
        ] {
            assert!(ui.restore_selection(invalid).is_err());
            assert_eq!(ui.selection, saved);
            assert_eq!(ui.scroll, scroll);
            assert_eq!(ui.focus, focus_target);
            assert!(ui.dragging);
        }
        ui.restore_selection(saved.clone()).unwrap();
        assert_eq!(ui.selection, saved);
        assert!(!ui.dragging);
    }

    #[test]
    fn goto_search_and_snapshot_restore_share_one_canonical_model() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.select_range(cell(3, 3), cell(1, 1)).unwrap();
        let goto = ui.selection.clone();
        assert_eq!(goto.anchor, Some(cell(1, 1)));
        assert_eq!(goto.focus, Some(cell(3, 3)));
        ui.select_cell(cell(2, 4)).unwrap();
        assert_eq!(ui.selection.anchor, Some(cell(2, 4)));
        assert_eq!(ui.selection.focus, Some(cell(2, 4)));
        ui.restore_selection(goto.clone()).unwrap();
        assert_eq!(ui.selection, goto);
        let mut other = ui.snapshot.clone();
        other.active_sheet = SheetId(1);
        ui.sync(other).unwrap();
        assert_eq!(ui.selection, SelectionState::default());
        assert!(ui.restore_selection(goto.clone()).is_err());
        assert_eq!(ui.selection, SelectionState::default());
        let mut original = ui.snapshot.clone();
        original.active_sheet = SheetId(0);
        ui.sync(original).unwrap();
        ui.restore_selection(goto.clone()).unwrap();
        assert_eq!(ui.selection, goto);
    }

    #[test]
    fn control_a_bounds_sparse_known_cells_and_merges_without_materializing_range() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        let mut snapshot = ui.snapshot.clone();
        snapshot.cells = [cell(2, 1), cell(5, 3)]
            .into_iter()
            .map(|address| UiCell {
                address,
                display: "known".into(),
                raw: "known".into(),
                format: None,
            })
            .collect();
        ui.sync(snapshot).unwrap();
        ui.apply_sheet_metadata(&[], &[], &[CellRange::new(cell(1, 2), cell(7, 5)).unwrap()])
            .unwrap();
        let before = ui.snapshot.clone();
        let event = UiEvent::KeyDown {
            key: Key::Character('a'),
            modifiers: Modifiers {
                control: true,
                ..Modifiers::default()
            },
        };
        assert!(ui.handle_event(event.clone()).unwrap().is_empty());
        assert_eq!(
            ui.selection.range(),
            Some(CellRange::new(cell(1, 1), cell(7, 5)).unwrap())
        );
        assert_eq!(ui.snapshot, before);
        let saved = ui.selection.clone();
        ui.set_focus_target(FocusTarget::FormulaBar);
        ui.handle_event(event).unwrap();
        assert_eq!(ui.selection, saved);
    }

    #[test]
    fn huge_logical_range_keeps_sparse_data_and_visible_frame_bounded() {
        let mut ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 1_000_000,
                columns: 16_384,
            },
            ..UiConfig::default()
        })
        .unwrap();
        let before = ui.snapshot.clone();
        ui.select_range(cell(0, 0), cell(999_999, 16_383)).unwrap();
        let selection = ui.selection.clone();
        ui.restore_selection(selection.clone()).unwrap();
        assert_eq!(ui.snapshot, before);
        assert_eq!(ui.selection, selection);
        let frame = ui.frame().unwrap();
        assert!(frame.cells.len() < 1_000);
        assert!(
            frame
                .cells
                .iter()
                .any(|rendered| rendered.address == cell(999_999, 16_383))
        );
    }

    #[test]
    fn empty_grid_navigation_and_control_a_do_not_create_invalid_selection() {
        let mut ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 0,
                columns: 0,
            },
            ..UiConfig::default()
        })
        .unwrap();
        shift_key(&mut ui, Key::ArrowRight);
        ui.handle_event(UiEvent::KeyDown {
            key: Key::Character('A'),
            modifiers: Modifiers {
                control: true,
                ..Modifiers::default()
            },
        })
        .unwrap();
        assert_eq!(ui.selection, SelectionState::default());
        assert!(ui.frame().unwrap().cells.is_empty());
    }

    #[test]
    fn replacing_merge_metadata_recomputes_extent_from_directional_endpoints() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        let big = CellRange::new(cell(0, 0), cell(3, 3)).unwrap();
        ui.apply_sheet_metadata(&[], &[], std::slice::from_ref(&big))
            .unwrap();
        ui.select_cell(cell(1, 1)).unwrap();
        assert_eq!(ui.selection.range(), Some(big));
        let smaller = CellRange::new(cell(1, 1), cell(1, 2)).unwrap();
        ui.apply_sheet_metadata(&[], &[], std::slice::from_ref(&smaller))
            .unwrap();
        assert_eq!(ui.selection.range(), Some(smaller));
        assert_eq!(ui.selection.anchor, Some(cell(1, 1)));
        assert_eq!(ui.selection.focus, Some(cell(1, 1)));
        ui.apply_sheet_metadata(&[], &[], &[]).unwrap();
        assert_eq!(
            ui.selection.range(),
            Some(CellRange::new(cell(1, 1), cell(1, 1)).unwrap())
        );
        assert_eq!(ui.selection.kind, SelectionKind::Cell);
    }

    #[test]
    fn merged_input_reads_and_commits_use_anchor_without_moving_selection_focus() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        let mut snapshot = ui.snapshot.clone();
        snapshot.cells = vec![UiCell {
            address: cell(0, 0),
            display: "original".into(),
            raw: "original".into(),
            format: None,
        }];
        ui.sync(snapshot).unwrap();
        ui.apply_sheet_metadata(&[], &[], &[CellRange::new(cell(0, 0), cell(1, 1)).unwrap()])
            .unwrap();
        ui.select_cell(cell(1, 1)).unwrap();
        assert_eq!(ui.active_input_address(), Some(cell(0, 0)));
        assert_eq!(ui.frame().unwrap().formula_text, "original");
        ui.handle_event(UiEvent::FormulaBarInput("replacement".into()))
            .unwrap();
        assert_eq!(
            ui.handle_event(UiEvent::FormulaBarCommit).unwrap(),
            vec![UiEffect::CommitCellRaw {
                address: cell(0, 0),
                raw: "replacement".into()
            }]
        );
        let point = point_for_cell(&ui, cell(1, 1));
        ui.handle_event(UiEvent::PointerDoubleClick {
            point,
            button: PointerButton::Primary,
        })
        .unwrap();
        assert_eq!(ui.edit_session().unwrap().address, cell(0, 0));
        ui.handle_event(UiEvent::TextInput("edited".into()))
            .unwrap();
        assert_eq!(
            ui.handle_event(UiEvent::KeyDown {
                key: Key::Enter,
                modifiers: Modifiers::default()
            })
            .unwrap(),
            vec![UiEffect::CommitCellRaw {
                address: cell(0, 0),
                raw: "edited".into()
            }]
        );
        assert_eq!(ui.selection.focus, Some(cell(1, 1)));
        ui.handle_event(UiEvent::KeyDown {
            key: Key::Escape,
            modifiers: Modifiers::default(),
        })
        .unwrap();
        ui.handle_event(UiEvent::KeyDown {
            key: Key::F2,
            modifiers: Modifiers::default(),
        })
        .unwrap();
        assert_eq!(ui.edit_session().unwrap().address, cell(0, 0));
    }

    #[test]
    fn public_focus_changes_cancel_drag_and_keep_text_field_shortcuts_owned() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        pointer_down(&mut ui, cell(1, 1), false);
        ui.set_focus_target(FocusTarget::NameBox);
        assert!(!ui.dragging);
        let saved = ui.selection.clone();
        pointer_up(&mut ui, cell(2, 2));
        for key in [Key::Character('a'), Key::ArrowRight] {
            assert!(
                ui.handle_event(UiEvent::KeyDown {
                    key,
                    modifiers: Modifiers {
                        control: true,
                        ..Modifiers::default()
                    },
                })
                .unwrap()
                .is_empty()
            );
            assert_eq!(ui.selection, saved);
        }
        ui.set_focus_target(FocusTarget::Grid);
        ui.handle_event(UiEvent::KeyDown {
            key: Key::ArrowRight,
            modifiers: Modifiers::default(),
        })
        .unwrap();
        assert_eq!(ui.selection.focus, Some(cell(1, 2)));
        ui.handle_event(UiEvent::KeyDown {
            key: Key::F2,
            modifiers: Modifiers::default(),
        })
        .unwrap();
        ui.handle_event(UiEvent::TextInput("grid draft".into()))
            .unwrap();
        ui.set_focus_target(FocusTarget::NameBox);
        assert!(
            ui.handle_event(UiEvent::KeyDown {
                key: Key::Enter,
                modifiers: Modifiers::default(),
            })
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn pane_clips_match_frozen_hit_testing_and_do_not_overlap_headers() {
        let mut ui = SheetsUi::new(UiConfig {
            frozen: FrozenPanes::with(1, 1),
            default_column_width: 80,
            default_row_height: 40,
            ..UiConfig::default()
        })
        .unwrap();
        ui.set_zoom(2.0);
        ui.scroll = ScrollOffset { x: 500, y: 150 };
        let frame = ui.frame().unwrap();
        let freeze_x = frame.layout.grid_viewport.x + 160.0;
        let freeze_y = frame.layout.grid_viewport.y + 80.0;
        for rendered in &frame.cells {
            if let Some(visible) = rendered.visible_rect {
                if rendered.address.column.0 == 0 {
                    assert!(visible.right() <= freeze_x);
                } else {
                    assert!(visible.x >= freeze_x);
                }
                if rendered.address.row.0 == 0 {
                    assert!(visible.bottom() <= freeze_y);
                } else {
                    assert!(visible.y >= freeze_y);
                }
                let point = Point::new(visible.x + visible.w / 2.0, visible.y + visible.h / 2.0);
                assert_eq!(ui.hit_test_grid(point), Some(rendered.address));
            }
        }
        for header in frame
            .column_headers
            .iter()
            .filter(|header| header.index > 0)
        {
            if let Some(visible) = header.visible_rect {
                assert!(visible.x >= freeze_x);
            }
        }
        for header in frame.row_headers.iter().filter(|header| header.index > 0) {
            if let Some(visible) = header.visible_rect {
                assert!(visible.y >= freeze_y);
            }
        }
        let overlapping_body = frame
            .cells
            .iter()
            .find(|cell| cell.address == self::cell(4, 7))
            .unwrap();
        assert!(overlapping_body.rect.x < freeze_x);
        assert!(overlapping_body.visible_rect.unwrap().x >= freeze_x);
    }

    #[test]
    fn fractional_inverse_zoom_keeps_last_physical_strip_clickable() {
        for zoom in [0.6, 0.7, 1.5, 2.0] {
            let mut ui = SheetsUi::new(UiConfig {
                outer_width: 173,
                outer_height: 225,
                ..UiConfig::default()
            })
            .unwrap();
            ui.set_zoom(zoom);
            let viewport = ui.layout.grid_viewport;
            let point = Point::new(viewport.right() - 0.25, viewport.bottom() - 0.25);
            let expected = cell(
                ((point.y - viewport.y) / f64::from(zoom) / f64::from(ui.config.default_row_height))
                    as u32,
                ((point.x - viewport.x)
                    / f64::from(zoom)
                    / f64::from(ui.config.default_column_width)) as u32,
            );
            assert_eq!(ui.hit_test_grid(point), Some(expected));
            assert_eq!(
                ui.hit_test_grid(Point::new(viewport.right(), viewport.bottom())),
                None
            );
        }
    }

    #[test]
    fn new_allows_zero_dimensions() {
        let ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 0,
                columns: 0,
            },
            ..UiConfig::default()
        });
        assert!(ui.is_ok());
    }

    #[test]
    fn new_rejects_zero_cell_sizes() {
        assert!(matches!(
            SheetsUi::new(UiConfig {
                default_row_height: 0,
                ..UiConfig::default()
            }),
            Err(UiError::InvalidConfig(_))
        ));
    }

    #[test]
    fn new_initializes_default_selection() {
        let ui = SheetsUi::new(UiConfig::default()).unwrap();
        assert_eq!(ui.selection().kind, SelectionKind::None);
        assert_eq!(ui.active_sheet(), SheetId(0));
    }

    #[test]
    fn select_range_canonicalizes_reversed_endpoints_and_scrolls_to_start() {
        let mut ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 100_000,
                columns: 16_384,
            },
            ..UiConfig::default()
        })
        .unwrap();
        let far = CellAddress::new(SheetId(0), RowIndex(99_999), ColumnIndex(16_383));
        let near = CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(0));

        ui.select_range(far, near).unwrap();

        assert_eq!(ui.selection.kind, SelectionKind::Range);
        assert_eq!(ui.selection.anchor, Some(near));
        assert_eq!(ui.selection.focus, Some(far));
        assert_eq!(ui.selection.range().unwrap().start, near);
        assert_eq!(ui.selection.range().unwrap().end, far);
        assert_eq!(ui.focus, FocusTarget::Grid);
        assert_eq!(ui.scroll_offset(), ScrollOffset::default());

        ui.select_range(near, near).unwrap();
        assert_eq!(ui.selection.kind, SelectionKind::Cell);
        assert_eq!(ui.selection.anchor, Some(near));
        assert_eq!(ui.selection.focus, Some(near));
    }

    #[test]
    fn select_range_rejects_invalid_addresses_without_changing_selection_or_scroll() {
        let mut ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 4,
                columns: 4,
            },
            ..UiConfig::default()
        })
        .unwrap();
        let a1 = CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(0));
        let b2 = CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(1));
        ui.selection = SelectionState {
            kind: SelectionKind::Range,
            anchor: Some(a1),
            focus: Some(b2),
            extent: None,
        };
        ui.scroll = ScrollOffset { x: 123, y: 456 };
        ui.focus = FocusTarget::FormulaBar;
        let old_selection = ui.selection.clone();
        let old_scroll = ui.scroll_offset();
        let old_focus = ui.focus;

        let other_sheet = CellAddress::new(SheetId(1), RowIndex(1), ColumnIndex(1));
        assert!(matches!(
            ui.select_range(a1, other_sheet),
            Err(UiError::InvalidAddress(_))
        ));
        assert_eq!(ui.selection, old_selection);
        assert_eq!(ui.scroll_offset(), old_scroll);
        assert_eq!(ui.focus, old_focus);

        let out_of_bounds = CellAddress::new(SheetId(0), RowIndex(4), ColumnIndex(0));
        assert!(matches!(
            ui.select_range(a1, out_of_bounds),
            Err(UiError::Grid(GridError::OutOfBounds { .. }))
        ));
        assert_eq!(ui.selection, old_selection);
        assert_eq!(ui.scroll_offset(), old_scroll);
        assert_eq!(ui.focus, old_focus);
    }

    #[test]
    fn select_range_has_visible_positive_geometry_without_mutating_document_data() {
        let mut api = SpreadsheetApi::<sheets_api::IroncalcAdapter>::production();
        let sheet = api.document().active_sheet();
        let a1 = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        let b2 = CellAddress::new(sheet, RowIndex(1), ColumnIndex(1));
        api.set_cell_input(sheet, a1, CellInput::Number(7.0))
            .unwrap();
        let document_before = api.document().snapshot();
        let snapshot = UiSnapshot::from_api_document(&api).unwrap();
        let mut ui = SheetsUi::new(UiConfig {
            initial_sheet: sheet,
            dimensions: snapshot.dimensions,
            ..UiConfig::default()
        })
        .unwrap();
        ui.sync(snapshot).unwrap();

        ui.select_range(a1, b2).unwrap();

        let frame = ui.frame().unwrap();
        let selected: Vec<_> = frame.cells.iter().filter(|cell| cell.selected).collect();
        assert_eq!(selected.len(), 4);
        let viewport = frame.layout.grid_viewport;
        for cell in selected {
            assert!(cell.rect.w > 0.0);
            assert!(cell.rect.h > 0.0);
            assert!(cell.rect.x >= viewport.x);
            assert!(cell.rect.y >= viewport.y);
            assert!(cell.rect.x + cell.rect.w <= viewport.x + viewport.w);
            assert!(cell.rect.y + cell.rect.h <= viewport.y + viewport.h);
        }
        assert_eq!(api.document().snapshot(), document_before);
    }

    #[test]
    fn sync_validates_addresses() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        let bad = UiCell {
            address: CellAddress::new(SheetId(1), RowIndex(0), ColumnIndex(0)),
            display: "x".into(),
            raw: "x".into(),
            format: None,
        };
        let snapshot = UiSnapshot {
            active_sheet: SheetId(0),
            dimensions: GridDimensions {
                rows: 100_000,
                columns: 16_384,
            },
            cells: vec![bad],
            sheets: vec![],
            status_text: String::new(),
            frozen: FrozenPanes::default(),
        };
        assert!(matches!(ui.sync(snapshot), Err(UiError::InvalidAddress(_))));
    }

    #[test]
    fn sync_updates_snapshot() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        let snapshot = UiSnapshot {
            active_sheet: SheetId(0),
            dimensions: GridDimensions {
                rows: 100_000,
                columns: 16_384,
            },
            cells: vec![UiCell {
                address: CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(0)),
                display: "1".into(),
                raw: "1".into(),
                format: None,
            }],
            sheets: vec![UiSheet {
                id: SheetId(0),
                name: "S".into(),
                active: true,
                used_cells: 1,
                merges: 0,
            }],
            status_text: String::new(),
            frozen: FrozenPanes::default(),
        };
        ui.sync(snapshot).unwrap();
        assert_eq!(ui.snapshot.cells.len(), 1);
    }

    #[test]
    fn resize_recomputes_layout() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.handle_event(UiEvent::Resize {
            width: 1024,
            height: 768,
        })
        .unwrap();
        assert_eq!(ui.layout().grid_viewport.w, (1024f32 - 48.0 - 14.0) as f64);
    }

    #[test]
    fn resize_keeps_selected_cell_inside_the_actual_grid_viewport() {
        let mut ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 200,
                columns: 30,
            },
            outer_width: 1180,
            outer_height: 590,
            ..UiConfig::default()
        })
        .unwrap();
        let target = CellAddress::new(SheetId(0), RowIndex(99), ColumnIndex(25));
        ui.select_cell(target).unwrap();

        ui.handle_event(UiEvent::Resize {
            width: 924,
            height: 197,
        })
        .unwrap();

        let frame = ui.frame().unwrap();
        let selected = frame
            .cells
            .iter()
            .find(|cell| cell.address == target)
            .expect("the selected cell must be in the reflowed frame");
        let viewport = frame.layout.grid_viewport;
        assert!(selected.rect.x >= viewport.x);
        assert!(selected.rect.y >= viewport.y);
        assert!(selected.rect.x + selected.rect.w <= viewport.x + viewport.w);
        assert!(selected.rect.y + selected.rect.h <= viewport.y + viewport.h);
    }
    #[test]
    fn key_right_moves_selection() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.handle_event(UiEvent::KeyDown {
            key: Key::ArrowRight,
            modifiers: Modifiers::default(),
        })
        .unwrap();
        assert_eq!(
            ui.selection().focus,
            Some(CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(1)))
        );
    }

    #[test]
    fn key_down_beyond_bottom_clamps() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.selection = SelectionState {
            kind: SelectionKind::None,
            anchor: None,
            focus: Some(CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(0))),
            extent: None,
        };
        for _ in 0..100_001 {
            ui.handle_event(UiEvent::KeyDown {
                key: Key::ArrowDown,
                modifiers: Modifiers::default(),
            })
            .unwrap();
        }
        assert_eq!(
            ui.selection().focus,
            Some(CellAddress::new(
                SheetId(0),
                RowIndex(99_999),
                ColumnIndex(0)
            ))
        );
    }

    #[test]
    fn wheel_updates_scroll_offset() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.handle_event(UiEvent::Wheel {
            delta_x: 5,
            delta_y: 10,
        })
        .unwrap();
        assert_eq!(ui.scroll_offset().x, 5);
        assert_eq!(ui.scroll_offset().y, 10);
    }

    #[test]
    fn double_click_opens_edit() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.handle_event(UiEvent::PointerDoubleClick {
            point: point_for_cell(&ui, cell(0, 0)),
            button: PointerButton::Primary,
        })
        .unwrap();
        assert!(ui.edit_session().is_some());
    }

    #[test]
    fn formula_commit_emits_effect() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.selection = SelectionState {
            kind: SelectionKind::None,
            anchor: None,
            focus: Some(CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(0))),
            extent: None,
        };
        ui.formula_draft = "=1+1".into();
        let effects = ui.handle_event(UiEvent::FormulaBarCommit).unwrap();
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, UiEffect::CommitCellRaw { raw, .. } if raw == "=1+1"))
        );
    }

    #[test]
    fn activate_sheet_emits_effect() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        let effects = ui.handle_event(UiEvent::ActivateSheet(SheetId(1))).unwrap();
        assert_eq!(effects, vec![UiEffect::ActivateSheet(SheetId(1))]);
    }

    #[test]
    fn toggle_formatting_panel_updates_state() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.handle_event(UiEvent::ToggleFormattingPanel).unwrap();
        assert!(ui.layout().formatting_panel.is_some());
    }

    #[test]
    fn frame_has_tabs_and_status() {
        let ui = SheetsUi::new(UiConfig::default()).unwrap();
        let frame = ui.frame().unwrap();
        assert!(frame.layout.sheet_tabs.h > 0.0);
        assert!(frame.layout.status_bar.h > 0.0);
    }

    #[test]
    fn frame_builds_column_labels_and_name_box() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.selection = SelectionState {
            kind: SelectionKind::Cell,
            anchor: Some(CellAddress::new(SheetId(0), RowIndex(4), ColumnIndex(2))),
            focus: Some(CellAddress::new(SheetId(0), RowIndex(4), ColumnIndex(2))),
            extent: None,
        };
        let frame = ui.frame().unwrap();
        assert_eq!(frame.column_headers[0].label, "A");
        assert_eq!(frame.column_headers[1].label, "B");
        assert_eq!(frame.column_headers[2].label, "C");
        assert_eq!(frame.name_box, "C5");
    }

    #[test]
    fn frame_for_100k_rows_is_bounded() {
        let ui = SheetsUi::new(UiConfig::default()).unwrap();
        let frame = ui.frame().unwrap();
        assert!(frame.cells.len() < 10_000);
        assert!(frame.row_headers.len() < 100_000);
        assert!(frame.column_headers.len() < 16_384);
    }

    #[test]
    fn zero_dimension_frame_is_empty() {
        let ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 0,
                columns: 0,
            },
            ..UiConfig::default()
        })
        .unwrap();
        let frame = ui.frame().unwrap();
        assert!(frame.cells.is_empty());
        assert!(frame.row_headers.is_empty());
        assert!(frame.column_headers.is_empty());
    }

    #[test]
    fn theme_defaults_are_set() {
        let theme = UiTheme::default();
        assert_eq!(theme.text, Rgba8::rgb(31, 35, 41));
    }

    #[test]
    fn parse_a1_range_parses_single_cell() {
        assert_eq!(
            parse_a1_range(
                "a1",
                SheetId(2),
                GridDimensions {
                    rows: 10,
                    columns: 10,
                }
            ),
            Ok((
                CellAddress::new(SheetId(2), RowIndex(0), ColumnIndex(0)),
                CellAddress::new(SheetId(2), RowIndex(0), ColumnIndex(0)),
            ))
        );
    }

    #[test]
    fn parse_a1_range_parses_multi_letter_column() {
        assert_eq!(
            parse_a1_range(
                "$AA$10",
                SheetId(0),
                GridDimensions {
                    rows: 10,
                    columns: 27,
                }
            ),
            Ok((
                CellAddress::new(SheetId(0), RowIndex(9), ColumnIndex(26)),
                CellAddress::new(SheetId(0), RowIndex(9), ColumnIndex(26)),
            ))
        );
    }

    #[test]
    fn parse_a1_range_canonicalizes_reversed_range() {
        assert_eq!(
            parse_a1_range(
                "B2:a1",
                SheetId(0),
                GridDimensions {
                    rows: 2,
                    columns: 2,
                }
            ),
            Ok((
                CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(0)),
                CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(1)),
            ))
        );
    }

    #[test]
    fn parse_a1_range_rejects_zero_row() {
        assert!(matches!(
            parse_a1_range(
                "A0",
                SheetId(0),
                GridDimensions {
                    rows: 1,
                    columns: 1,
                }
            ),
            Err(UiError::InvalidInput(_))
        ));
    }

    #[test]
    fn parse_a1_range_rejects_out_of_bounds() {
        assert!(matches!(
            parse_a1_range(
                "C3",
                SheetId(0),
                GridDimensions {
                    rows: 2,
                    columns: 2,
                }
            ),
            Err(UiError::Grid(GridError::OutOfBounds { .. }))
        ));
    }

    #[test]
    fn ui_cell_uses_display_and_raw_text() {
        let mut api = SpreadsheetApi::<sheets_api::IroncalcAdapter>::production();
        let sheet = api.document().active_sheet();
        let source = CellAddress::new(sheet, RowIndex(0), ColumnIndex(0));
        let address = CellAddress::new(sheet, RowIndex(0), ColumnIndex(1));
        api.set_cell_input(sheet, source, CellInput::Number(1.0))
            .unwrap();
        api.set_cell_input(
            sheet,
            address,
            CellInput::formula("SUM(A1)".to_string()).unwrap(),
        )
        .unwrap();
        let view = api.cell_view(sheet, address).unwrap();
        let cell: UiCell = view.into();
        assert_eq!(cell.display, "1");
        assert_eq!(cell.raw, "=SUM(A1)");
    }

    #[test]
    fn from_api_imports_snapshot_into_ui() {
        let mut source = SpreadsheetApi::<sheets_api::IroncalcAdapter>::production();
        let source_sheet = source.document().active_sheet();
        let a1 = CellAddress::new(source_sheet, RowIndex(0), ColumnIndex(0));
        let b1 = CellAddress::new(source_sheet, RowIndex(0), ColumnIndex(1));
        let c1 = CellAddress::new(source_sheet, RowIndex(0), ColumnIndex(2));
        source
            .set_cell_input(source_sheet, a1, CellInput::Number(1.0))
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
        source.add_sheet(Some("Calc".into())).unwrap();
        source.set_active_sheet(source_sheet).unwrap();
        let bytes = source.export_xlsx().unwrap();

        let mut imported = SpreadsheetApi::<sheets_api::IroncalcAdapter>::production();
        let snapshot = UiSnapshot::from_api(&mut imported, &bytes).unwrap();

        let summaries = &snapshot.sheets;
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, source_sheet);
        assert_eq!(summaries[0].name, "Sheet1");
        assert_eq!(summaries[1].name, "Calc");
        assert_eq!(snapshot.active_sheet, imported.document().active_sheet());
        assert_eq!(snapshot.active_sheet, source_sheet);

        let config = imported.config();
        assert_eq!(snapshot.dimensions.rows, config.max_document_rows);
        assert_eq!(snapshot.dimensions.columns, config.max_document_columns);
        assert_eq!(
            snapshot.status_text,
            format!(
                "revision={} sheets={} active={}",
                imported.document().revision(),
                imported.document().sheet_count(),
                imported.document().active_sheet()
            )
        );
        assert!(
            snapshot
                .cells
                .iter()
                .all(|cell| cell.address.sheet == snapshot.active_sheet)
        );
        let formula = snapshot
            .cells
            .iter()
            .find(|cell| cell.address == c1)
            .expect("imported formula cell");
        assert_eq!(formula.display, "3");
        assert_eq!(formula.raw, "=A1+B1");

        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.sync(snapshot).unwrap();
        let frame = ui.frame().unwrap();
        assert!(
            frame
                .cells
                .iter()
                .any(|cell| cell.address == c1 && cell.display == "3")
        );
        assert_eq!(frame.status_text, ui.snapshot.status_text);
    }
    #[test]
    fn tab_and_enter_move_active_cell() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.handle_event(UiEvent::KeyDown {
            key: Key::Tab,
            modifiers: Modifiers::default(),
        })
        .unwrap();
        ui.handle_event(UiEvent::KeyDown {
            key: Key::Enter,
            modifiers: Modifiers::default(),
        })
        .unwrap();
        assert_eq!(
            ui.selection().focus,
            Some(CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(1)))
        );
    }

    #[test]
    fn zoom_out_expands_visible_range_without_unnecessary_scroll() {
        let mut ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 40,
                columns: 4,
            },
            outer_width: 924,
            outer_height: 197,
            ..UiConfig::default()
        })
        .unwrap();
        let target = CellAddress::new(SheetId(0), RowIndex(5), ColumnIndex(1));

        let at_100 = ui.frame().unwrap();
        assert!(at_100.cells.iter().all(|cell| cell.address != target));

        ui.set_zoom(0.5);
        let at_50 = ui.frame().unwrap();
        assert!(at_50.cells.iter().any(|cell| cell.address == target));

        ui.select_cell(target).unwrap();
        assert_eq!(ui.scroll_offset().y, 0);
    }
    #[test]
    fn zoom_scales_visible_cells() {
        let ui = SheetsUi::new(UiConfig::default()).unwrap();
        let base = ui.frame().unwrap();
        let base_width = base.cells[0].rect.w;
        let mut zoomed = SheetsUi::new(UiConfig::default()).unwrap();
        zoomed.set_zoom(2.0);
        let frame = zoomed.frame().unwrap();
        assert_eq!(frame.zoom, 2.0);
        assert!((frame.cells[0].rect.w - base_width * 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn metadata_preserves_format_dimensions_and_merges() {
        let mut ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 4,
                columns: 4,
            },
            ..UiConfig::default()
        })
        .unwrap();
        let a1 = CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(0));
        let b2 = CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(1));
        let snapshot = UiSnapshot {
            active_sheet: SheetId(0),
            dimensions: GridDimensions {
                rows: 4,
                columns: 4,
            },
            cells: vec![UiCell {
                address: a1,
                display: "styled".into(),
                raw: "styled".into(),
                format: Some(CellFormat::default()),
            }],
            sheets: vec![],
            status_text: String::new(),
            frozen: FrozenPanes::default(),
        };
        ui.sync(snapshot).unwrap();
        let merge = CellRange::new(a1, b2).unwrap();
        ui.apply_sheet_metadata(
            &[RowDimension {
                row: RowIndex(0),
                height: 40.0,
            }],
            &[ColumnDimension {
                column: ColumnIndex(0),
                width: 140.0,
            }],
            std::slice::from_ref(&merge),
        )
        .unwrap();
        let frame = ui.frame().unwrap();
        let cell = frame.cells.iter().find(|cell| cell.address == a1).unwrap();
        assert_eq!(cell.rect.h, 40.0);
        assert_eq!(cell.rect.w, 140.0);
        assert!(cell.format.is_some());
        assert_eq!(frame.merges, vec![merge]);
    }

    #[test]
    fn projection_defaults_follow_document_metrics() {
        let document_config = sheets_api::DocumentConfig::default();
        let config = UiConfig::default();
        assert_eq!(
            config.default_row_height,
            u32::from(document_config.default_row_height)
        );
        assert_eq!(
            config.default_column_width,
            u32::from(document_config.default_column_width)
        );
        let frame = SheetsUi::new(config.clone()).unwrap().frame().unwrap();
        assert_eq!(frame.cells[0].rect.h, f64::from(config.default_row_height));
        assert_eq!(
            frame.cells[0].rect.w,
            f64::from(config.default_column_width)
        );
    }

    #[test]
    fn projection_merge_keeps_offscreen_anchor_and_full_variable_axis_geometry() {
        let dimensions = GridDimensions {
            rows: 100,
            columns: 100,
        };
        let mut ui = SheetsUi::new(UiConfig {
            dimensions,
            outer_width: 400,
            outer_height: 300,
            default_row_height: 20,
            default_column_width: 30,
            ..UiConfig::default()
        })
        .unwrap();
        let anchor_address = CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(1));
        let anchor = UiCell {
            address: anchor_address,
            display: "anchor display".into(),
            raw: "=SUM(A1:A2)".into(),
            format: Some(CellFormat::default()),
        };
        ui.sync(UiSnapshot {
            active_sheet: SheetId(0),
            dimensions,
            cells: vec![anchor.clone()],
            sheets: Vec::new(),
            status_text: String::new(),
            frozen: FrozenPanes::default(),
        })
        .unwrap();
        let merge = CellRange::new(
            anchor_address,
            CellAddress::new(SheetId(0), RowIndex(20), ColumnIndex(20)),
        )
        .unwrap();
        let distant_merge = CellRange::new(
            CellAddress::new(SheetId(0), RowIndex(80), ColumnIndex(80)),
            CellAddress::new(SheetId(0), RowIndex(81), ColumnIndex(81)),
        )
        .unwrap();
        ui.apply_sheet_metadata(
            &[
                RowDimension {
                    row: RowIndex(1),
                    height: 40.0,
                },
                RowDimension {
                    row: RowIndex(5),
                    height: 50.0,
                },
                RowDimension {
                    row: RowIndex(15),
                    height: 100.0,
                },
            ],
            &[
                ColumnDimension {
                    column: ColumnIndex(1),
                    width: 70.0,
                },
                ColumnDimension {
                    column: ColumnIndex(5),
                    width: 90.0,
                },
                ColumnDimension {
                    column: ColumnIndex(15),
                    width: 120.0,
                },
            ],
            &[merge.clone(), distant_merge.clone()],
        )
        .unwrap();
        ui.scroll = ScrollOffset { x: 250, y: 150 };
        ui.set_zoom(1.5);

        let frame = ui.frame().unwrap();
        assert!(
            frame
                .cells
                .iter()
                .all(|cell| cell.address != anchor_address)
        );
        assert!(frame.row_headers.iter().all(|header| header.index != 15));
        assert!(frame.column_headers.iter().all(|header| header.index != 15));
        assert_eq!(frame.merges, vec![merge.clone(), distant_merge]);
        assert_eq!(frame.merged_cells.len(), 1);
        let projected = &frame.merged_cells[0];
        assert_eq!(projected.range, merge);
        assert_eq!(projected.anchor, anchor);
        assert_eq!(projected.rect, Rect::new(-282.0, -139.0, 1185.0, 795.0));
        assert_eq!(projected.visible_rect, frame.layout.grid_viewport);
        assert_eq!(frame.scroll_x, 375.0);
        assert_eq!(frame.scroll_y, 225.0);
    }

    #[test]
    fn projection_merge_clips_partial_range_and_preserves_empty_anchor() {
        let mut ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 10,
                columns: 10,
            },
            outer_width: 400,
            outer_height: 300,
            default_row_height: 20,
            default_column_width: 30,
            ..UiConfig::default()
        })
        .unwrap();
        let merge = CellRange::new(
            CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(1)),
            CellAddress::new(SheetId(0), RowIndex(3), ColumnIndex(4)),
        )
        .unwrap();
        ui.apply_sheet_metadata(&[], &[], std::slice::from_ref(&merge))
            .unwrap();
        ui.scroll = ScrollOffset { x: 50, y: 35 };
        ui.set_zoom(2.0);
        let frame = ui.frame().unwrap();
        let projected = &frame.merged_cells[0];
        assert_eq!(projected.rect, Rect::new(8.0, 26.0, 240.0, 120.0));
        assert_eq!(projected.visible_rect, Rect::new(48.0, 56.0, 200.0, 90.0));
        assert_eq!(projected.anchor.address, merge.start);
        assert!(projected.anchor.display.is_empty());
        assert!(projected.anchor.raw.is_empty());
        assert!(projected.anchor.format.is_none());

        ui.scroll = ScrollOffset { x: 150, y: 80 };
        assert!(ui.frame().unwrap().merged_cells.is_empty());
    }

    #[test]
    fn projection_scrolled_cell_preserves_signed_origin_and_one_pass_zoom() {
        let mut ui = SheetsUi::new(UiConfig {
            dimensions: GridDimensions {
                rows: 10,
                columns: 10,
            },
            default_row_height: 100,
            default_column_width: 140,
            ..UiConfig::default()
        })
        .unwrap();
        ui.scroll = ScrollOffset { x: 100, y: 80 };
        ui.set_zoom(2.0);
        let frame = ui.frame().unwrap();
        let first = frame
            .cells
            .iter()
            .find(|cell| cell.address.row == RowIndex(0) && cell.address.column == ColumnIndex(0))
            .unwrap();
        assert_eq!(first.rect, Rect::new(-152.0, -104.0, 280.0, 200.0));
        assert!(
            first
                .rect
                .intersection(frame.layout.grid_viewport)
                .is_some()
        );
    }

    #[test]
    fn negative_wheel_moves_back_towards_origin() {
        let mut ui = SheetsUi::new(UiConfig::default()).unwrap();
        ui.handle_event(UiEvent::Wheel {
            delta_x: 12,
            delta_y: 20,
        })
        .unwrap();
        ui.handle_event(UiEvent::Wheel {
            delta_x: -5,
            delta_y: -7,
        })
        .unwrap();
        assert_eq!(ui.scroll_offset().x, 7);
        assert_eq!(ui.scroll_offset().y, 13);
    }
}
