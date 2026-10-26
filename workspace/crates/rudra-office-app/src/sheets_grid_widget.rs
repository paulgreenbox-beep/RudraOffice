use makepad_widgets::widget::WidgetActionData;
use makepad_widgets::*;
use sheets_core::{
    Border, BorderStyle, CellAddress, CellFormat, CellRange, HorizontalAlign, SheetId,
    VerticalAlign,
};
use sheets_ui::{CellRenderModel, SheetTabRenderModel, UiFrame};

const FORMULA_BAR_HEIGHT: f64 = 32.0;
const COLUMN_HEADER_HEIGHT: f64 = 24.0;
const ROW_HEADER_WIDTH: f64 = 48.0;
const SHEET_TABS_HEIGHT: f64 = 32.0;
const MIN_SCROLLBAR_THUMB: f64 = 28.0;
const CELL_TEXT_CAP: usize = 256;

pub(crate) fn sheets_modifiers(modifiers: KeyModifiers) -> sheets_ui::Modifiers {
    sheets_ui::Modifiers {
        shift: modifiers.shift,
        control: modifiers.control,
        alt: modifiers.alt,
        meta: modifiers.logo,
    }
}

#[derive(Clone, Debug, Default)]
pub enum SheetsGridAction {
    #[default]
    None,
    SheetClicked(SheetId),
    Wheel {
        delta_x: i64,
        delta_y: i64,
        control: bool,
    },
    SelectionInput(sheets_ui::UiEvent),
    KeyDown(KeyCode, KeyModifiers),
    ZoomIn,
    ZoomOut,
}

#[derive(Script, ScriptHook, Widget)]
pub struct SheetsGridCanvas {
    #[uid]
    uid: WidgetUid,
    #[source]
    source: ScriptObjectRef,
    #[walk]
    walk: Walk,
    #[layout]
    layout: Layout,

    #[redraw]
    #[live]
    draw_bg: DrawColor,
    #[live]
    draw_chrome: DrawColor,
    #[live]
    draw_cell: DrawColor,
    #[live]
    draw_grid: DrawColor,
    #[live]
    draw_selection: DrawColor,
    #[live]
    draw_text: DrawText,
    #[live]
    draw_text_bold: DrawText,
    #[live]
    draw_text_italic: DrawText,
    #[live]
    draw_text_bold_italic: DrawText,

    #[action_data]
    #[rust]
    action_data: WidgetActionData,
    #[rust]
    frame: Option<UiFrame>,
    /// Visual indication only; canonical pending Cut belongs to AppState/API.
    #[rust]
    pending_cut: Option<CellRange>,
    /// Last non-zero canvas size sent to the pure Sheets layout. Keeping this
    /// on the widget prevents redraw-based resize loops.
    #[rust]
    last_reported_viewport: Option<(u32, u32)>,
    /// Native capture lifecycle only; anchor/range belong exclusively to SheetsUi.
    #[rust]
    selecting: bool,
}

impl SheetsGridCanvas {
    pub fn set_pending_cut(&mut self, cx: &mut Cx, range: Option<CellRange>) {
        if self.pending_cut != range {
            self.pending_cut = range;
            self.draw_bg.redraw(cx);
        }
    }
    pub fn set_frame(&mut self, cx: &mut Cx, frame: Option<UiFrame>) {
        let active_sheet =
            |frame: &UiFrame| frame.tabs.iter().find(|tab| tab.active).map(|tab| tab.id);
        if frame.is_none()
            || self.frame.as_ref().and_then(active_sheet) != frame.as_ref().and_then(active_sheet)
        {
            self.cancel_selection(cx);
        }
        self.frame = frame;
        self.draw_bg.redraw(cx);
    }

    /// Returns the real Makepad canvas size once after it changes. It is only
    /// called from AppMain after a Draw pass, when this area is initialized.
    pub fn take_viewport_change(&mut self, cx: &Cx) -> Option<(u32, u32)> {
        let area = self.draw_bg.area();
        if self.frame.is_none() || !area.is_valid(cx) {
            return None;
        }
        let rect = area.rect(cx);
        if !rect.size.x.is_finite() || !rect.size.y.is_finite() {
            return None;
        }
        let width = rect.size.x.round().max(0.0) as u32;
        let height = rect.size.y.round().max(0.0) as u32;
        if width == 0 || height == 0 {
            return None;
        }
        let viewport = (width, height);
        if self.last_reported_viewport == Some(viewport) {
            return None;
        }
        self.last_reported_viewport = Some(viewport);
        Some(viewport)
    }

    fn emit(&self, cx: &mut Cx, action: SheetsGridAction) {
        cx.widget_action_with_data(&self.action_data, self.uid, action);
    }

    pub fn cancel_selection(&mut self, cx: &mut Cx) {
        if self.selecting {
            self.selecting = false;
            self.emit(
                cx,
                SheetsGridAction::SelectionInput(sheets_ui::UiEvent::PointerLeave),
            );
        }
    }

    fn pointer_point(canvas: Rect, abs: DVec2) -> sheets_grid::Point {
        sheets_grid::Point {
            x: abs.x - canvas.pos.x,
            y: abs.y - canvas.pos.y,
        }
    }

    fn local_cell_rect(&self, _frame: &UiFrame, cell: &CellRenderModel, canvas: Rect) -> Rect {
        Rect {
            // UiFrame already owns zoom and scroll. Only translate to the host.
            pos: canvas.pos + dvec2(cell.rect.x, cell.rect.y),
            size: dvec2(cell.rect.w, cell.rect.h),
        }
    }

    fn grid_rect(frame: &UiFrame, canvas: Rect) -> Rect {
        let grid = frame.layout.grid_viewport;
        Rect {
            pos: canvas.pos + dvec2(grid.x, grid.y),
            size: dvec2(grid.w, grid.h),
        }
    }

    fn tab_at(&self, frame: &UiFrame, point: DVec2, canvas: Rect) -> Option<SheetId> {
        let y = canvas.pos.y + canvas.size.y - SHEET_TABS_HEIGHT;
        if point.y < y || point.y > y + SHEET_TABS_HEIGHT {
            return None;
        }
        let mut x = canvas.pos.x;
        for tab in &frame.tabs {
            let width = tab_width(tab);
            if point.x >= x && point.x <= x + width {
                return Some(tab.id);
            }
            x += width;
        }
        None
    }

    fn merged_anchor(frame: &UiFrame, address: CellAddress) -> Option<CellAddress> {
        frame.merges.iter().find_map(|merge| {
            if merge.sheet == address.sheet
                && address.row.0 >= merge.start.row.0
                && address.row.0 <= merge.end.row.0
                && address.column.0 >= merge.start.column.0
                && address.column.0 <= merge.end.column.0
            {
                Some(merge.start)
            } else {
                None
            }
        })
    }

    fn merged_rect(&self, frame: &UiFrame, cell: &CellRenderModel, canvas: Rect) -> Rect {
        self.local_cell_rect(frame, cell, canvas)
    }

    /// Merge geometry comes from the full pure grid, never from a union of
    /// visible cells. A visible tail still paints even when its anchor scrolled
    /// off screen, with the same text origin and bounds at every scroll offset.
    fn paint_cells(frame: &UiFrame) -> Vec<CellRenderModel> {
        let mut cells: Vec<_> = frame
            .cells
            .iter()
            .filter(|cell| {
                cell.visible_rect.is_some() && Self::merged_anchor(frame, cell.address).is_none()
            })
            .cloned()
            .collect();
        for merge in &frame.merged_cells {
            let address = merge.range.start;
            cells.push(CellRenderModel {
                address,
                rect: merge.rect,
                visible_rect: Some(merge.visible_rect),
                display: merge.anchor.display.clone(),
                format: merge.anchor.format.clone(),
                selected: frame.selection.contains(address),
                active: frame.selection.focus == Some(address),
                editing: frame
                    .editor
                    .as_ref()
                    .is_some_and(|edit| edit.address == address),
            });
        }
        cells
    }

    fn draw_box(&mut self, cx: &mut Cx2d, rect: Rect, color: Vec4f) {
        self.draw_cell.color = color;
        self.draw_cell.draw_abs(cx, rect);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text_at(
        &mut self,
        cx: &mut Cx2d,
        text: &str,
        x: f64,
        y: f64,
        color: Vec4f,
        size: f32,
        bold: bool,
    ) {
        let clipped = clip_text(text);
        let draw = if bold {
            &mut self.draw_text_bold
        } else {
            &mut self.draw_text
        };
        draw.color = color;
        draw.text_style.font_size = size;
        // Chrome call sites specify a baseline, while DrawText::draw_abs takes
        // a top-left origin. Convert using actual font metrics, not font size.
        let Some(run) = draw.prepare_single_line_run(cx, &clipped) else {
            return;
        };
        draw.draw_abs(cx, dvec2(x, y - run.ascender_in_lpxs as f64), &clipped);
    }

    fn draw_cell_text(
        &mut self,
        cx: &mut Cx2d,
        text: &str,
        rect: Rect,
        color: Vec4f,
        size: f32,
        format: Option<&CellFormat>,
    ) {
        let bold = format.is_some_and(|value| value.font.bold);
        let italic = format.is_some_and(|value| value.font.italic);
        let horizontal = format
            .and_then(|value| value.alignment.horizontal.clone())
            .unwrap_or(HorizontalAlign::Left);
        let vertical = format
            .and_then(|value| value.alignment.vertical.clone())
            .unwrap_or(VerticalAlign::Middle);
        let clipped = clip_text(text);
        if clipped.is_empty() || rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        let draw = match (bold, italic) {
            (true, true) => &mut self.draw_text_bold_italic,
            (true, false) => &mut self.draw_text_bold,
            (false, true) => &mut self.draw_text_italic,
            (false, false) => &mut self.draw_text,
        };
        draw.color = color;
        draw.text_style.font_size = size;
        draw.font_scale = 1.0;
        let padding = (size as f64 / 11.0 * 3.0).max(1.0);
        let inner = inset_rect(rect, padding);
        if inner.size.x <= 0.0 || inner.size.y <= 0.0 {
            return;
        }
        let wrap = format.is_some_and(|value| value.alignment.wrap);
        let laidout = draw.layout(
            cx,
            0.0,
            0.0,
            wrap.then_some(inner.size.x as f32),
            wrap,
            Align::default(),
            &clipped,
        );
        if laidout.rows.is_empty() {
            return;
        }
        let measured = dvec2(
            laidout.size_in_lpxs.width as f64,
            laidout.size_in_lpxs.height as f64,
        );
        let origin = aligned_text_origin(inner, measured, horizontal, vertical);
        cx.push_clip_rect(rect);
        draw.draw_walk_laidout(
            cx,
            Walk {
                abs_pos: Some(origin),
                width: Size::Fixed(measured.x),
                height: Size::Fixed(measured.y),
                ..Walk::default()
            },
            &laidout,
        );
        cx.pop_clip_rect();
    }

    fn draw_cell_borders(
        &mut self,
        cx: &mut Cx2d,
        rect: Rect,
        borders: &sheets_core::Borders,
        zoom: f32,
    ) {
        self.draw_border(cx, rect, borders.top.as_ref(), 0, zoom);
        self.draw_border(cx, rect, borders.right.as_ref(), 1, zoom);
        self.draw_border(cx, rect, borders.bottom.as_ref(), 2, zoom);
        self.draw_border(cx, rect, borders.left.as_ref(), 3, zoom);
    }

    fn draw_border(
        &mut self,
        cx: &mut Cx2d,
        rect: Rect,
        border: Option<&Border>,
        edge: u8,
        zoom: f32,
    ) {
        let Some(border) = border else { return };
        if matches!(&border.style, BorderStyle::None) {
            return;
        }
        self.draw_grid.color = border
            .color
            .clone()
            .map(color)
            .unwrap_or(Vec4f::from_u32(0x667085ff));
        let line = cell_border_rect(rect, edge, zoom);
        self.draw_grid.draw_abs(cx, line);
    }
    fn draw_tabs(
        &mut self,
        cx: &mut Cx2d,
        frame: &UiFrame,
        canvas: Rect,
        text: Vec4f,
        chrome: Vec4f,
        focus: Vec4f,
    ) {
        let y = canvas.pos.y + canvas.size.y - SHEET_TABS_HEIGHT;
        let mut x = canvas.pos.x;
        for tab in &frame.tabs {
            let width = tab_width(tab);
            self.draw_box(
                cx,
                Rect {
                    pos: dvec2(x, y),
                    size: dvec2(width - 1.0, SHEET_TABS_HEIGHT - 1.0),
                },
                if tab.active {
                    Vec4f::from_u32(0xffffffff)
                } else {
                    chrome
                },
            );
            self.draw_text_at(
                cx,
                &tab.name,
                x + 12.0,
                y + 21.0,
                if tab.active { focus } else { text },
                12.0,
                tab.active,
            );
            x += width;
        }
    }
}

impl Widget for SheetsGridCanvas {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, _scope: &mut Scope) {
        let area = self.draw_bg.area();
        if matches!(
            event,
            Event::MouseLeave(_) | Event::WindowLostFocus(_) | Event::WindowGeomChange(_)
        ) {
            self.cancel_selection(cx);
        }
        if self.frame.is_none() || !area.is_valid(cx) {
            return;
        }
        match event.hits_with_capture_overload(cx, area, true) {
            Hit::FingerDown(fe) if fe.is_primary_hit() => {
                cx.set_key_focus(area);
                let canvas = area.rect(cx);
                let point = fe.abs;
                if let Some(frame) = self.frame.as_ref() {
                    if let Some(sheet) = self.tab_at(frame, point, canvas) {
                        self.selecting = false;
                        self.emit(cx, SheetsGridAction::SheetClicked(sheet));
                    } else if zoom_control_rects(canvas).2.contains(point) {
                        self.emit(cx, SheetsGridAction::ZoomIn);
                    } else if zoom_control_rects(canvas).0.contains(point) {
                        self.emit(cx, SheetsGridAction::ZoomOut);
                    } else if Self::grid_rect(frame, canvas).contains(point) {
                        self.selecting = true;
                        self.emit(
                            cx,
                            SheetsGridAction::SelectionInput(sheets_ui::UiEvent::PointerDown {
                                point: Self::pointer_point(canvas, point),
                                button: sheets_ui::PointerButton::Primary,
                                modifiers: sheets_modifiers(fe.modifiers),
                            }),
                        );
                    }
                }
            }
            Hit::FingerMove(fe) if self.selecting => {
                let canvas = area.rect(cx);
                if !fe.is_over {
                    self.cancel_selection(cx);
                } else {
                    self.emit(
                        cx,
                        SheetsGridAction::SelectionInput(sheets_ui::UiEvent::PointerMove {
                            point: Self::pointer_point(canvas, fe.abs),
                            modifiers: sheets_modifiers(fe.modifiers),
                        }),
                    );
                }
            }
            Hit::FingerUp(fe) if self.selecting => {
                self.selecting = false;
                self.emit(
                    cx,
                    SheetsGridAction::SelectionInput(sheets_ui::UiEvent::PointerUp {
                        point: Self::pointer_point(area.rect(cx), fe.abs),
                        button: sheets_ui::PointerButton::Primary,
                        modifiers: sheets_modifiers(fe.modifiers),
                    }),
                );
            }
            Hit::FingerScroll(fe) => {
                self.emit(
                    cx,
                    SheetsGridAction::Wheel {
                        delta_x: fe.scroll.x.round() as i64,
                        delta_y: fe.scroll.y.round() as i64,
                        control: fe.modifiers.control,
                    },
                );
            }
            Hit::KeyDown(ke) => {
                self.emit(cx, SheetsGridAction::KeyDown(ke.key_code, ke.modifiers));
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let canvas = cx.walk_turtle(walk);
        self.draw_bg.color = Vec4f::from_u32(0xffffffff);
        self.draw_bg.draw_abs(cx, canvas);
        let Some(frame) = self.frame.clone() else {
            return DrawStep::done();
        };
        cx.push_clip_rect(canvas);

        let text = Vec4f::from_u32(0x1f2329ff);
        let muted = Vec4f::from_u32(0x5c6370ff);
        let chrome = Vec4f::from_u32(0xf3f5f7ff);
        let grid = Vec4f::from_u32(0xdadde2ff);
        let selection = Vec4f::from_u32(0x1e68d2ff);

        self.draw_chrome.color = chrome;
        self.draw_chrome.draw_abs(
            cx,
            Rect {
                pos: canvas.pos,
                size: dvec2(canvas.size.x, FORMULA_BAR_HEIGHT),
            },
        );
        self.draw_box(
            cx,
            Rect {
                pos: canvas.pos,
                size: dvec2(96.0, FORMULA_BAR_HEIGHT),
            },
            Vec4f::from_u32(0xe8edf3ff),
        );
        self.draw_text_at(
            cx,
            if frame.name_box.is_empty() {
                "A1"
            } else {
                &frame.name_box
            },
            canvas.pos.x + 10.0,
            canvas.pos.y + 21.0,
            text,
            13.0,
            false,
        );
        cx.push_clip_rect(Rect {
            pos: canvas.pos + dvec2(96.0, 0.0),
            size: dvec2((canvas.size.x - 232.0).max(0.0), FORMULA_BAR_HEIGHT),
        });
        self.draw_text_at(
            cx,
            &format!("fx  {}", frame.formula_text),
            canvas.pos.x + 108.0,
            canvas.pos.y + 21.0,
            muted,
            13.0,
            false,
        );
        cx.pop_clip_rect();
        let (minus, percent, plus) = zoom_control_rects(canvas);
        let mut centered = CellFormat::default();
        centered.alignment.horizontal = Some(HorizontalAlign::Center);
        centered.alignment.vertical = Some(VerticalAlign::Middle);
        self.draw_cell_text(cx, "−", minus, text, 14.0, Some(&centered));
        self.draw_cell_text(
            cx,
            &format!("{}%", (frame.zoom * 100.0).round() as u32),
            percent,
            text,
            11.0,
            Some(&centered),
        );
        self.draw_cell_text(cx, "+", plus, text, 14.0, Some(&centered));

        let corner = Rect {
            pos: dvec2(canvas.pos.x, canvas.pos.y + FORMULA_BAR_HEIGHT),
            size: dvec2(ROW_HEADER_WIDTH, COLUMN_HEADER_HEIGHT),
        };
        self.draw_chrome.draw_abs(cx, corner);
        let grid_rect = Self::grid_rect(&frame, canvas);
        cx.push_clip_rect(Rect {
            pos: dvec2(grid_rect.pos.x, canvas.pos.y + FORMULA_BAR_HEIGHT),
            size: dvec2(grid_rect.size.x, COLUMN_HEADER_HEIGHT),
        });
        for header in &frame.column_headers {
            let Some(clip) = header.visible_rect else {
                continue;
            };
            cx.push_clip_rect(Rect {
                pos: canvas.pos + dvec2(clip.x, clip.y),
                size: dvec2(clip.w, clip.h),
            });
            let rect = Rect {
                pos: dvec2(
                    canvas.pos.x + header.rect.x,
                    canvas.pos.y + FORMULA_BAR_HEIGHT,
                ),
                size: dvec2(header.rect.w, COLUMN_HEADER_HEIGHT),
            };
            self.draw_chrome.draw_abs(cx, rect);
            self.draw_cell_text(
                cx,
                &header.label,
                rect,
                if header.selected { selection } else { muted },
                11.0,
                None,
            );
            cx.pop_clip_rect();
        }
        cx.pop_clip_rect();
        cx.push_clip_rect(Rect {
            pos: dvec2(canvas.pos.x, grid_rect.pos.y),
            size: dvec2(ROW_HEADER_WIDTH, grid_rect.size.y),
        });
        for header in &frame.row_headers {
            let Some(clip) = header.visible_rect else {
                continue;
            };
            cx.push_clip_rect(Rect {
                pos: canvas.pos + dvec2(clip.x, clip.y),
                size: dvec2(clip.w, clip.h),
            });
            let rect = Rect {
                pos: dvec2(canvas.pos.x, canvas.pos.y + header.rect.y),
                size: dvec2(ROW_HEADER_WIDTH, header.rect.h),
            };
            self.draw_chrome.draw_abs(cx, rect);
            self.draw_cell_text(
                cx,
                &header.label,
                rect,
                if header.selected { selection } else { muted },
                11.0,
                None,
            );
            cx.pop_clip_rect();
        }
        cx.pop_clip_rect();

        cx.push_clip_rect(grid_rect);
        let painted = Self::paint_cells(&frame);
        for cell in &painted {
            let Some(clip) = cell.visible_rect else {
                continue;
            };
            if Self::merged_anchor(&frame, cell.address)
                .is_some_and(|anchor| anchor != cell.address)
            {
                continue;
            }
            let rect = self.merged_rect(&frame, cell, canvas);
            if intersect_rect(rect, grid_rect).is_none() {
                continue;
            }
            cx.push_clip_rect(Rect {
                pos: canvas.pos + dvec2(clip.x, clip.y),
                size: dvec2(clip.w, clip.h),
            });
            let background = cell
                .format
                .as_ref()
                .and_then(|format| format.background.clone())
                .map(color)
                .unwrap_or(Vec4f::from_u32(0xffffffff));
            self.draw_box(
                cx,
                rect,
                if cell.selected && !cell.active {
                    Vec4f::from_u32(0xddebffff)
                } else {
                    background
                },
            );
            self.draw_grid.color = grid;
            self.draw_grid.draw_abs(
                cx,
                Rect {
                    pos: dvec2(rect.pos.x, rect.pos.y + rect.size.y - 1.0),
                    size: dvec2(rect.size.x, 1.0),
                },
            );
            self.draw_grid.draw_abs(
                cx,
                Rect {
                    pos: dvec2(rect.pos.x + rect.size.x - 1.0, rect.pos.y),
                    size: dvec2(1.0, rect.size.y),
                },
            );
            let foreground = cell
                .format
                .as_ref()
                .and_then(|format| format.foreground.clone())
                .map(color)
                .unwrap_or(text);
            let size = cell
                .format
                .as_ref()
                .map(|format| format.font.size as f32)
                .unwrap_or(11.0)
                .clamp(8.0, 32.0)
                * frame.zoom;
            self.draw_cell_text(
                cx,
                &cell.display,
                rect,
                foreground,
                size,
                cell.format.as_ref(),
            );
            if cell.active {
                self.draw_selection.color = selection;
                self.draw_selection.draw_abs(
                    cx,
                    Rect {
                        pos: dvec2(rect.pos.x, rect.pos.y),
                        size: dvec2(rect.size.x, 2.0),
                    },
                );
                self.draw_selection.draw_abs(
                    cx,
                    Rect {
                        pos: dvec2(rect.pos.x, rect.pos.y + rect.size.y - 2.0),
                        size: dvec2(rect.size.x, 2.0),
                    },
                );
                self.draw_selection.draw_abs(
                    cx,
                    Rect {
                        pos: dvec2(rect.pos.x, rect.pos.y),
                        size: dvec2(2.0, rect.size.y),
                    },
                );
                self.draw_selection.draw_abs(
                    cx,
                    Rect {
                        pos: dvec2(rect.pos.x + rect.size.x - 2.0, rect.pos.y),
                        size: dvec2(2.0, rect.size.y),
                    },
                );
            }
            if let Some(range) = &self.pending_cut
                && range.contains(&cell.address)
            {
                // Static green boundary, clipped by the same sole grid owner.
                // No timer/animation and no off-screen cells are materialized.
                self.draw_selection.color = Vec4f::from_u32(0x2f8c62ff);
                let edges = [
                    (
                        cell.address.row == range.start.row,
                        rect.pos,
                        dvec2(rect.size.x, 2.0),
                    ),
                    (
                        cell.address.row == range.end.row,
                        dvec2(rect.pos.x, rect.pos.y + rect.size.y - 2.0),
                        dvec2(rect.size.x, 2.0),
                    ),
                    (
                        cell.address.column == range.start.column,
                        rect.pos,
                        dvec2(2.0, rect.size.y),
                    ),
                    (
                        cell.address.column == range.end.column,
                        dvec2(rect.pos.x + rect.size.x - 2.0, rect.pos.y),
                        dvec2(2.0, rect.size.y),
                    ),
                ];
                for (visible, pos, size) in edges {
                    if visible {
                        self.draw_selection.draw_abs(cx, Rect { pos, size });
                    }
                }
            }
            cx.pop_clip_rect();
        }
        // Borders share cell edges. Paint after all fills so adjacent cells neither
        // erase the edge nor contribute a second inward stroke.
        for cell in &painted {
            let rect = self.merged_rect(&frame, cell, canvas);
            if intersect_rect(rect, grid_rect).is_some()
                && let Some(format) = cell.format.as_ref()
                && let Some(clip) = cell.visible_rect
            {
                cx.push_clip_rect(Rect {
                    pos: canvas.pos + dvec2(clip.x, clip.y),
                    size: dvec2(clip.w, clip.h),
                });
                self.draw_cell_borders(cx, rect, &format.borders, frame.zoom);
                cx.pop_clip_rect();
            }
        }
        cx.pop_clip_rect();

        let scrollbar_y = canvas.pos.y + canvas.size.y - SHEET_TABS_HEIGHT - 9.0;
        self.draw_chrome.draw_abs(
            cx,
            Rect {
                pos: dvec2(canvas.pos.x + ROW_HEADER_WIDTH, scrollbar_y),
                size: dvec2((canvas.size.x - ROW_HEADER_WIDTH - 14.0).max(0.0), 8.0),
            },
        );
        self.draw_chrome.draw_abs(
            cx,
            Rect {
                pos: dvec2(
                    canvas.pos.x + canvas.size.x - 9.0,
                    canvas.pos.y + FORMULA_BAR_HEIGHT + COLUMN_HEADER_HEIGHT,
                ),
                size: dvec2(
                    8.0,
                    (canvas.size.y
                        - FORMULA_BAR_HEIGHT
                        - COLUMN_HEADER_HEIGHT
                        - SHEET_TABS_HEIGHT
                        - 14.0)
                        .max(0.0),
                ),
            },
        );
        let horizontal_track = (canvas.size.x - ROW_HEADER_WIDTH - 14.0).max(1.0);
        let horizontal_view = frame.layout.grid_viewport.w.max(1.0);
        if let Some(thumb) = scrollbar_thumb(horizontal_track, horizontal_view, frame.content_width)
        {
            let max_scroll = (frame.content_width - horizontal_view).max(1.0);
            let thumb_x = canvas.pos.x
                + ROW_HEADER_WIDTH
                + (horizontal_track - thumb) * (frame.scroll_x / max_scroll).clamp(0.0, 1.0);
            self.draw_selection.color = Vec4f::from_u32(0x91a4b8ff);
            self.draw_selection.draw_abs(
                cx,
                Rect {
                    pos: dvec2(thumb_x, scrollbar_y),
                    size: dvec2(thumb, 8.0),
                },
            );
        }
        let vertical_track =
            (canvas.size.y - FORMULA_BAR_HEIGHT - COLUMN_HEADER_HEIGHT - SHEET_TABS_HEIGHT - 14.0)
                .max(1.0);
        let vertical_view = frame.layout.grid_viewport.h.max(1.0);
        if let Some(thumb) = scrollbar_thumb(vertical_track, vertical_view, frame.content_height) {
            let max_scroll = (frame.content_height - vertical_view).max(1.0);
            let thumb_y = canvas.pos.y
                + FORMULA_BAR_HEIGHT
                + COLUMN_HEADER_HEIGHT
                + (vertical_track - thumb) * (frame.scroll_y / max_scroll).clamp(0.0, 1.0);
            self.draw_selection.color = Vec4f::from_u32(0x91a4b8ff);
            self.draw_selection.draw_abs(
                cx,
                Rect {
                    pos: dvec2(canvas.pos.x + canvas.size.x - 9.0, thumb_y),
                    size: dvec2(8.0, thumb),
                },
            );
        }
        self.draw_tabs(cx, &frame, canvas, text, chrome, selection);
        cx.pop_clip_rect();
        DrawStep::done()
    }
}

fn tab_width(tab: &SheetTabRenderModel) -> f64 {
    (tab.name.chars().count() as f64 * 8.0 + 32.0).clamp(88.0, 220.0)
}

fn cell_border_rect(rect: Rect, edge: u8, zoom: f32) -> Rect {
    let thickness = (zoom as f64).clamp(0.5, 4.0);
    let half = thickness * 0.5;
    match edge {
        0 => Rect {
            pos: rect.pos - dvec2(0.0, half),
            size: dvec2(rect.size.x, thickness),
        },
        1 => Rect {
            pos: dvec2(rect.pos.x + rect.size.x - half, rect.pos.y),
            size: dvec2(thickness, rect.size.y),
        },
        2 => Rect {
            pos: dvec2(rect.pos.x, rect.pos.y + rect.size.y - half),
            size: dvec2(rect.size.x, thickness),
        },
        _ => Rect {
            pos: rect.pos - dvec2(half, 0.0),
            size: dvec2(thickness, rect.size.y),
        },
    }
}

fn zoom_control_rects(canvas: Rect) -> (Rect, Rect, Rect) {
    let control = |offset: f64, width: f64| Rect {
        pos: dvec2(
            canvas.pos.x + (canvas.size.x - offset).max(0.0),
            canvas.pos.y,
        ),
        size: dvec2(width, FORMULA_BAR_HEIGHT),
    };
    (
        control(130.0, 32.0),
        control(98.0, 56.0),
        control(42.0, 32.0),
    )
}

fn inset_rect(rect: Rect, padding: f64) -> Rect {
    Rect {
        pos: rect.pos + dvec2(padding, padding),
        size: dvec2(
            (rect.size.x - padding * 2.0).max(0.0),
            (rect.size.y - padding * 2.0).max(0.0),
        ),
    }
}

fn aligned_text_origin(
    inner: Rect,
    measured: DVec2,
    horizontal: HorizontalAlign,
    vertical: VerticalAlign,
) -> DVec2 {
    let remaining = dvec2(
        (inner.size.x - measured.x).max(0.0),
        (inner.size.y - measured.y).max(0.0),
    );
    inner.pos
        + dvec2(
            match horizontal {
                HorizontalAlign::Left => 0.0,
                HorizontalAlign::Center => remaining.x * 0.5,
                HorizontalAlign::Right => remaining.x,
            },
            match vertical {
                VerticalAlign::Top => 0.0,
                VerticalAlign::Middle => remaining.y * 0.5,
                VerticalAlign::Bottom => remaining.y,
            },
        )
}

fn intersect_rect(left: Rect, right: Rect) -> Option<Rect> {
    let pos = dvec2(left.pos.x.max(right.pos.x), left.pos.y.max(right.pos.y));
    let end = dvec2(
        (left.pos.x + left.size.x).min(right.pos.x + right.size.x),
        (left.pos.y + left.size.y).min(right.pos.y + right.size.y),
    );
    let size = end - pos;
    (size.x > 0.0 && size.y > 0.0).then_some(Rect { pos, size })
}

fn scrollbar_thumb(track: f64, viewport: f64, content: f64) -> Option<f64> {
    if !track.is_finite()
        || !viewport.is_finite()
        || !content.is_finite()
        || track <= 0.0
        || viewport <= 0.0
        || content <= viewport
    {
        return None;
    }

    let minimum = MIN_SCROLLBAR_THUMB.min(track);
    Some((track * viewport / content).clamp(minimum, track))
}

fn clip_text(text: &str) -> String {
    let mut result: String = text.chars().take(CELL_TEXT_CAP).collect();
    if text.chars().count() > CELL_TEXT_CAP {
        result.push('…');
    }
    result
}

fn color(value: sheets_core::Color) -> Vec4f {
    Vec4f {
        x: value.r as f32 / 255.0,
        y: value.g as f32 / 255.0,
        z: value.b as f32 / 255.0,
        w: value.a as f32 / 255.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_cell_border_is_single_stroke_and_scales_with_zoom() {
        let left = Rect {
            pos: dvec2(48.0, 56.0),
            size: dvec2(90.0, 25.0),
        };
        let right = Rect {
            pos: dvec2(138.0, 56.0),
            size: left.size,
        };
        for zoom in [0.5, 1.0, 2.0] {
            let edge = cell_border_rect(left, 1, zoom);
            assert_eq!(edge, cell_border_rect(right, 3, zoom));
            assert_eq!(edge.size.x, zoom as f64);
            assert_eq!(edge.pos.x + edge.size.x * 0.5, 138.0);
        }
    }

    #[test]
    fn cell_text_origin_uses_measured_height_not_baseline() {
        let inner = inset_rect(
            Rect {
                pos: dvec2(48.0, 56.0),
                size: dvec2(96.0, 25.0),
            },
            3.0,
        );
        let origin = aligned_text_origin(
            inner,
            dvec2(30.0, 14.0),
            HorizontalAlign::Right,
            VerticalAlign::Middle,
        );
        assert_eq!(origin, dvec2(111.0, 61.5));
        assert!(origin.y + 14.0 < 81.0);
    }

    #[test]
    fn wide_half_zoom_paints_all_visible_cells_without_prefix_truncation() {
        let mut view = sheets_ui::SheetsUi::new(sheets_ui::UiConfig {
            initial_sheet: SheetId(1),
            outer_width: 1920,
            outer_height: 1080,
            ..Default::default()
        })
        .unwrap();
        view.set_zoom(0.5);
        let frame = view.frame().unwrap();
        let visible = frame
            .cells
            .iter()
            .filter(|cell| cell.visible_rect.is_some())
            .count();
        assert!(
            visible > 2_500,
            "fixture must reproduce the old renderer limit"
        );
        let painted = SheetsGridCanvas::paint_cells(&frame);
        assert_eq!(painted.len(), visible);
        assert!(painted.iter().all(|cell| cell.visible_rect.is_some()));
        assert!(painted.iter().any(|cell| {
            cell.address
                == frame
                    .cells
                    .iter()
                    .rev()
                    .find(|cell| cell.visible_rect.is_some())
                    .unwrap()
                    .address
        }));
    }

    #[test]
    fn wide_half_zoom_keeps_merged_cells_after_the_visible_body_queue() {
        let sheet = SheetId(1);
        let mut view = sheets_ui::SheetsUi::new(sheets_ui::UiConfig {
            initial_sheet: sheet,
            outer_width: 1920,
            outer_height: 1080,
            ..Default::default()
        })
        .unwrap();
        view.set_zoom(0.5);
        let range = sheets_core::CellRange::new(
            CellAddress::new(
                sheet,
                sheets_core::RowIndex(70),
                sheets_core::ColumnIndex(2),
            ),
            CellAddress::new(
                sheet,
                sheets_core::RowIndex(71),
                sheets_core::ColumnIndex(3),
            ),
        )
        .unwrap();
        view.apply_sheet_metadata(&[], &[], std::slice::from_ref(&range))
            .unwrap();
        let frame = view.frame().unwrap();
        assert!(frame.cells.len() > 2_500);
        assert!(frame.merged_cells.iter().any(|cell| cell.range == range));
        let painted = SheetsGridCanvas::paint_cells(&frame);
        assert!(
            painted
                .iter()
                .any(|cell| cell.address == range.start && cell.visible_rect.is_some())
        );
        assert!(!painted.iter().any(|cell| cell.address == range.end));
    }

    #[test]
    fn zoom_buttons_and_percent_share_non_overlapping_draw_and_hit_rects() {
        let (minus, percent, plus) = zoom_control_rects(Rect {
            pos: dvec2(10.0, 20.0),
            size: dvec2(900.0, 600.0),
        });
        assert_eq!(minus.pos.x + minus.size.x, percent.pos.x);
        assert_eq!(percent.pos.x + percent.size.x, plus.pos.x);
        assert!(intersect_rect(minus, percent).is_none());
        assert!(intersect_rect(percent, plus).is_none());
    }

    #[test]
    fn oversized_wrapped_text_stays_at_content_top_and_is_clipped() {
        let inner = inset_rect(
            Rect {
                pos: dvec2(10.0, 20.0),
                size: dvec2(60.0, 24.0),
            },
            3.0,
        );
        assert_eq!(
            aligned_text_origin(
                inner,
                dvec2(100.0, 70.0),
                HorizontalAlign::Center,
                VerticalAlign::Bottom
            ),
            inner.pos
        );
    }

    #[test]
    fn scrolled_cell_is_clipped_to_grid_not_chrome() {
        let grid = Rect {
            pos: dvec2(48.0, 56.0),
            size: dvec2(500.0, 300.0),
        };
        let cell = Rect {
            pos: dvec2(28.0, 40.0),
            size: dvec2(96.0, 25.0),
        };
        let visible = intersect_rect(cell, grid).unwrap();
        assert_eq!(visible.pos, grid.pos);
        assert_eq!(visible.size, dvec2(76.0, 9.0));
        assert!(
            intersect_rect(
                Rect {
                    pos: dvec2(1.0, 1.0),
                    size: dvec2(10.0, 10.0)
                },
                grid
            )
            .is_none()
        );
    }

    #[test]
    fn merge_tail_paints_with_full_bounds_when_anchor_is_offscreen() {
        let mut frame = sheets_ui::SheetsUi::new(sheets_ui::UiConfig::default())
            .unwrap()
            .frame()
            .unwrap();
        let anchor = CellAddress::new(
            SheetId(0),
            sheets_core::RowIndex(0),
            sheets_core::ColumnIndex(0),
        );
        let range = sheets_core::CellRange::new(
            anchor,
            CellAddress::new(
                SheetId(0),
                sheets_core::RowIndex(2),
                sheets_core::ColumnIndex(3),
            ),
        )
        .unwrap();
        frame.merges = vec![range.clone()];
        let mut full = frame.cells[0].rect;
        full.x = -80.0;
        full.y = 40.0;
        full.w = 400.0;
        full.h = 75.0;
        let mut visible = full;
        visible.x = 48.0;
        visible.y = 56.0;
        visible.w = 272.0;
        visible.h = 59.0;
        frame.cells.clear();
        frame.merged_cells = vec![sheets_ui::UiMergedCell {
            range,
            anchor: sheets_ui::UiCell {
                address: anchor,
                display: "Объединение".into(),
                raw: "Объединение".into(),
                format: None,
            },
            rect: full,
            visible_rect: visible,
        }];
        let cells = SheetsGridCanvas::paint_cells(&frame);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].rect, full);
        assert_eq!(cells[0].address, anchor);
        assert_eq!(cells[0].display, "Объединение");
    }

    #[test]
    fn scrollbar_thumb_handles_tiny_initial_track_without_panicking() {
        assert_eq!(scrollbar_thumb(1.0, 1.0, 100.0), Some(1.0));
    }

    #[test]
    fn scrollbar_thumb_never_exceeds_short_track() {
        assert_eq!(scrollbar_thumb(20.0, 10.0, 1_000.0), Some(20.0));
    }

    #[test]
    fn scrollbar_thumb_uses_minimum_for_normal_track() {
        assert_eq!(scrollbar_thumb(100.0, 10.0, 1_000.0), Some(28.0));
    }

    #[test]
    fn scrollbar_thumb_is_hidden_when_content_fits() {
        assert_eq!(scrollbar_thumb(100.0, 100.0, 100.0), None);
    }

    #[test]
    fn scrollbar_thumb_rejects_non_finite_geometry() {
        assert_eq!(scrollbar_thumb(f64::NAN, 10.0, 100.0), None);
    }
}
const MAX_ACCESSIBILITY_CELLS: usize = 512;

/// A bounded, screen-space semantic projection of a visible sheet cell.
#[derive(Clone, Debug)]
pub(crate) struct SheetsGridAccessibilityCell {
    pub(crate) id_suffix: String,
    pub(crate) address: CellAddress,
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) bounds: Rect,
    pub(crate) selected: bool,
}

/// A visible sheet tab exposed without changing the grid's drawing ownership.
#[derive(Clone, Debug)]
pub(crate) struct SheetsGridAccessibilityTab {
    pub(crate) id_suffix: String,
    pub(crate) sheet: SheetId,
    pub(crate) name: String,
    pub(crate) bounds: Rect,
    pub(crate) active: bool,
}

/// The complete bounded semantic projection of the custom-drawn grid surface.
#[derive(Clone, Debug)]
pub(crate) struct SheetsGridAccessibilitySnapshot {
    pub(crate) bounds: Rect,
    pub(crate) cells: Vec<SheetsGridAccessibilityCell>,
    pub(crate) tabs: Vec<SheetsGridAccessibilityTab>,
    pub(crate) zoom_out_bounds: Rect,
    pub(crate) zoom_in_bounds: Rect,
}

impl SheetsGridCanvas {
    /// Returns only rendered/visible cells, capped to keep the UIA tree bounded.
    pub(crate) fn accessibility_snapshot(
        &self,
        cx: &Cx,
    ) -> Option<SheetsGridAccessibilitySnapshot> {
        let frame = self.frame.as_ref()?;
        let bounds = self.draw_bg.area().rect(cx);
        if bounds.size.x <= 0.0 || bounds.size.y <= 0.0 {
            return None;
        }

        let mut cells = Vec::new();
        for cell in &Self::paint_cells(frame) {
            if Self::merged_anchor(frame, cell.address).is_some_and(|anchor| anchor != cell.address)
            {
                continue;
            }
            if cell.display.trim().is_empty() && !cell.selected && !cell.active {
                continue;
            }
            let address = cell.address;
            let label = format!(
                "{}{}",
                super::column_label(address.column),
                address.row.0.saturating_add(1)
            );
            let display = clip_text(&cell.display);
            let Some(clip) = cell.visible_rect else {
                continue;
            };
            let visible_bounds = Rect {
                pos: bounds.pos + dvec2(clip.x, clip.y),
                size: dvec2(clip.w, clip.h),
            };
            cells.push(SheetsGridAccessibilityCell {
                id_suffix: format!(
                    "cell:{}:{}:{}",
                    address.sheet.0, address.row.0, address.column.0
                ),
                address,
                value: display.clone(),
                name: if display.is_empty() {
                    label
                } else {
                    format!("{label}: {display}")
                },
                bounds: visible_bounds,
                selected: cell.selected || cell.active,
            });
            if cells.len() == MAX_ACCESSIBILITY_CELLS {
                break;
            }
        }

        let mut tabs = Vec::new();
        let mut x = bounds.pos.x;
        let y = bounds.pos.y + bounds.size.y - SHEET_TABS_HEIGHT;
        for tab in &frame.tabs {
            let width = tab_width(tab);
            tabs.push(SheetsGridAccessibilityTab {
                id_suffix: format!("sheet-tab:{}", tab.id.0),
                sheet: tab.id,
                name: tab.name.clone(),
                bounds: Rect {
                    pos: dvec2(x, y),
                    size: dvec2((width - 1.0).max(0.0), SHEET_TABS_HEIGHT - 1.0),
                },
                active: tab.active,
            });
            x += width;
        }

        Some(SheetsGridAccessibilitySnapshot {
            bounds,
            cells,
            tabs,
            zoom_out_bounds: zoom_control_rects(bounds).0,
            zoom_in_bounds: zoom_control_rects(bounds).2,
        })
    }
}
