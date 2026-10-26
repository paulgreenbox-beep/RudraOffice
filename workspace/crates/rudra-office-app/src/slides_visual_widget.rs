use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use makepad_widgets::widget::WidgetActionData;
use makepad_widgets::*;
use slides_api::{
    Color, ImageFormat, ObjectId, ObjectKind, Paragraph, Point, Rect as SlideRect, Size, Text,
    TextAlignment,
};
use slides_canvas::{RectExt, RenderCommand, Viewport};
use slides_ui::{PresentationViewer, RenderPlan};

pub const THUMBNAIL_PANE_WIDTH: f32 = 168.0;
const THUMBNAIL_BOX_WIDTH: f32 = 136.0;
const THUMBNAIL_BOX_HEIGHT: f32 = 76.0;
const THUMBNAIL_ITEM_HEIGHT: f32 = 104.0;
const THUMBNAIL_TOP_PADDING: f32 = 12.0;
const THUMBNAIL_DRAW_TOP_INSET: f32 = 14.0;
// The frame remains bounded even in a tall window. The actual thumbnail
// window is further constrained by the measured canvas height because this
// lightweight strip has no independent vertical scroll offset.
const MAX_THUMBNAILS_IN_FRAME: usize = 12;
const MAX_TEXTURES: usize = 16;
// Sources are bounded before decoding; the resulting GPU texture is bounded
// independently so a high-resolution photograph can remain visible without
// making the active slide texture unbounded.
const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_IMAGE_SOURCE_PIXELS: usize = 24_000_000;
const MAX_TEXTURE_PIXELS: usize = 4_000_000;
const MAX_TEXT_CHARS: usize = 1_200;
const MAX_RENDER_COMMANDS: usize = 2_500;
const MAX_SEMANTIC_ITEMS: usize = 64;
const SEMANTIC_TEXT_CHAR_CAP: usize = 120;

#[derive(Clone, Debug, Default)]
pub enum SlidesVisualAction {
    #[default]
    None,
    ThumbnailClicked(usize),
    Wheel {
        delta_x: i64,
        delta_y: i64,
        control: bool,
    },
    KeyDown(KeyCode),
    TextObjectClicked(Option<ObjectId>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlidesThumbnailFrame {
    pub index: usize,
    pub selected: bool,
    pub plan: RenderPlan,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlidesTextHitTarget {
    pub object_id: ObjectId,
    pub rect: SlideRect,
}

/// Bounded, renderer-level accounting for the central slide render plan.
///
/// Counts describe commands, not source objects: a source table, for example,
/// expands into borders and text commands. This deliberately keeps source-model
/// and actual drawing evidence distinct.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SlidesRenderPlanSummary {
    pub total_commands: usize,
    pub considered_commands: usize,
    pub draw_commands: usize,
    pub text_commands: usize,
    pub image_commands: usize,
    pub graphic_commands: usize,
    pub line_commands: usize,
    /// Total rectangular clip commands in the bounded render plan.
    pub clip_commands: usize,
    /// `PopClip` without an owner, or `PushClip` without a matching bounded pop.
    pub unbalanced_clip_commands: usize,
    pub warning_count: usize,
    pub command_cap_reached: bool,
}

impl SlidesRenderPlanSummary {
    pub fn from_plan(plan: &RenderPlan) -> Self {
        let mut summary = Self {
            total_commands: plan.commands.len(),
            warning_count: plan.warnings.len(),
            command_cap_reached: plan.commands.len() > MAX_RENDER_COMMANDS,
            ..Self::default()
        };
        let mut clip_depth = 0usize;
        for command in plan.commands.iter().take(MAX_RENDER_COMMANDS) {
            summary.considered_commands += 1;
            match command {
                RenderCommand::Clear { .. } => summary.draw_commands += 1,
                RenderCommand::FillRect { .. }
                | RenderCommand::StrokeRect { .. }
                | RenderCommand::FillEllipse { .. }
                | RenderCommand::StrokeEllipse { .. }
                | RenderCommand::FillChevron { .. }
                | RenderCommand::StrokeChevron { .. } => {
                    summary.draw_commands += 1;
                    summary.graphic_commands += 1;
                }
                RenderCommand::DrawLine { .. } => {
                    summary.draw_commands += 1;
                    summary.line_commands += 1;
                }
                RenderCommand::DrawText { .. } => {
                    summary.draw_commands += 1;
                    summary.text_commands += 1;
                }
                RenderCommand::DrawImage { .. } => {
                    summary.draw_commands += 1;
                    summary.image_commands += 1;
                }
                RenderCommand::PushClip { .. } => {
                    summary.clip_commands += 1;
                    clip_depth += 1;
                }
                RenderCommand::PopClip => {
                    summary.clip_commands += 1;
                    if clip_depth == 0 {
                        summary.unbalanced_clip_commands += 1;
                    } else {
                        clip_depth -= 1;
                    }
                }
            }
        }
        summary.unbalanced_clip_commands += clip_depth;
        summary
    }

    pub fn needs_disclosure(self) -> bool {
        self.warning_count > 0 || self.command_cap_reached || self.unbalanced_clip_commands > 0
    }
}

/// What the Makepad canvas processed during its most recent central-slide draw.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SlidesDrawReport {
    pub plan: SlidesRenderPlanSummary,
    pub drawn_commands: usize,
    pub image_placeholders: usize,
    pub text_cap_truncations: usize,
    pub image_downsamples: usize,
    pub applied_clip_commands: usize,
    pub auto_closed_clip_commands: usize,
}

impl SlidesDrawReport {
    fn for_plan(plan: &RenderPlan) -> Self {
        Self {
            plan: SlidesRenderPlanSummary::from_plan(plan),
            ..Self::default()
        }
    }

    pub fn needs_disclosure(self) -> bool {
        self.plan.needs_disclosure()
            || self.image_placeholders > 0
            || self.image_downsamples > 0
            || self.text_cap_truncations > 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlidesAccessibilityKind {
    Text,
    Image,
    Graphic,
}

#[derive(Clone, Debug)]
pub struct SlidesAccessibilityItem {
    pub command_index: usize,
    pub kind: SlidesAccessibilityKind,
    pub name: String,
    pub bounds: Rect,
}

#[derive(Clone, Debug)]
pub struct SlidesVisualAccessibilitySnapshot {
    pub summary: String,
    pub draw_called: bool,
    pub items: Vec<SlidesAccessibilityItem>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct SlidesViewerFrame {
    pub current_index: usize,
    pub slide_count: usize,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub scale: f32,
    pub presentation_mode: bool,
    pub thumbnail_start: usize,
    pub thumbnail_end: usize,
    pub central: Option<RenderPlan>,
    pub central_plan_summary: Option<SlidesRenderPlanSummary>,
    pub text_hit_targets: Vec<SlidesTextHitTarget>,
    pub selected_text_object: Option<ObjectId>,
    pub thumbnails: Vec<SlidesThumbnailFrame>,
    pub warning_count: usize,
}

impl SlidesViewerFrame {
    pub fn from_viewer(
        viewer: &PresentationViewer,
        viewport_width: f32,
        viewport_height: f32,
        scale: f32,
        presentation_mode: bool,
    ) -> Self {
        let count = viewer.slide_count();
        let current = viewer
            .current_index()
            .unwrap_or(0)
            .min(count.saturating_sub(1));
        if count == 0 || viewport_width <= 1.0 || viewport_height <= 1.0 {
            return Self {
                current_index: current,
                slide_count: count,
                viewport_width,
                viewport_height,
                scale,
                presentation_mode,
                ..Self::default()
            };
        }

        let pane = if presentation_mode {
            0.0
        } else {
            THUMBNAIL_PANE_WIDTH
        };
        let central_width = (viewport_width - pane).max(1.0);
        let slide = &viewer.document().slides[current];
        let central_viewport =
            zoomed_fit_viewport(slide.size, central_width, viewport_height, scale);
        let text_hit_targets = slide
            .objects
            .iter()
            .filter(|object| matches!(&object.kind, ObjectKind::Text(_)))
            .filter_map(|object| {
                let rect = object
                    .bounds
                    .apply_rect(object.transform)
                    .apply_rect(central_viewport.transform);
                rect.is_valid().then_some(SlidesTextHitTarget {
                    object_id: object.id,
                    rect,
                })
            })
            .collect::<Vec<_>>();
        let selected_text_object = viewer.selected_object().filter(|object_id| {
            text_hit_targets
                .iter()
                .any(|target| target.object_id == *object_id)
        });
        let central = viewer.render_plan(current, central_viewport);
        let central_plan_summary = central.as_ref().map(SlidesRenderPlanSummary::from_plan);
        let (thumbnail_start, thumbnail_end) =
            visible_thumbnail_range_for_viewport(count, current, viewport_height);
        let thumbnails = if presentation_mode {
            Vec::new()
        } else {
            (thumbnail_start..thumbnail_end)
                .filter_map(|index| {
                    viewer
                        .thumbnail_plan(index, Size::new(THUMBNAIL_BOX_WIDTH, THUMBNAIL_BOX_HEIGHT))
                        .map(|plan| SlidesThumbnailFrame {
                            index,
                            selected: index == current,
                            plan,
                        })
                })
                .take(MAX_THUMBNAILS_IN_FRAME)
                .collect()
        };
        let warning_count = central
            .as_ref()
            .map(|plan| plan.warnings.len())
            .unwrap_or(0)
            + thumbnails
                .iter()
                .map(|item| item.plan.warnings.len())
                .sum::<usize>();
        Self {
            current_index: current,
            slide_count: count,
            viewport_width,
            viewport_height,
            scale,
            presentation_mode,
            thumbnail_start,
            thumbnail_end,
            central,
            central_plan_summary,
            text_hit_targets,
            selected_text_object,
            thumbnails,
            warning_count,
        }
    }

    pub fn status_text(&self) -> String {
        if self.slide_count == 0 {
            return "Слайды: нет данных".to_owned();
        }
        let mode = if self.presentation_mode {
            " · режим показа"
        } else {
            ""
        };
        let disclosure = self
            .central_plan_summary
            .map(|summary| {
                if summary.command_cap_reached {
                    " · часть содержимого вне лимита визуализации"
                } else if summary.warning_count > 0 {
                    " · часть содержимого отображена с заменой"
                } else if summary.unbalanced_clip_commands > 0 {
                    " · часть обрезки элементов некорректна"
                } else if summary.needs_disclosure() {
                    " · часть содержимого отображена упрощённо"
                } else {
                    ""
                }
            })
            .unwrap_or("");
        format!(
            "Слайд {}/{} · {:.0}%{}{}",
            self.current_index + 1,
            self.slide_count,
            self.scale * 100.0,
            mode,
            disclosure
        )
    }

    pub fn needs_viewport_reframe(&self, viewport_width: f32, viewport_height: f32) -> bool {
        if self.slide_count == 0 || viewport_width <= 1.0 || viewport_height <= 1.0 {
            return false;
        }
        self.viewport_width <= 1.0
            || self.viewport_height <= 1.0
            || (self.viewport_width - viewport_width).abs() > 0.5
            || (self.viewport_height - viewport_height).abs() > 0.5
    }
}

fn visible_thumbnail_range_for_viewport(
    count: usize,
    current: usize,
    viewport_height: f32,
) -> (usize, usize) {
    visible_thumbnail_range_with_window(
        count,
        current,
        thumbnail_slots_for_viewport(viewport_height),
    )
}

fn visible_thumbnail_range_with_window(
    count: usize,
    current: usize,
    requested_window: usize,
) -> (usize, usize) {
    if count == 0 {
        return (0, 0);
    }
    let window = requested_window
        .clamp(1, MAX_THUMBNAILS_IN_FRAME)
        .min(count);
    let max_start = count - window;
    let start = current
        .min(count.saturating_sub(1))
        .saturating_sub(window / 2)
        .min(max_start);
    (start, start + window)
}

fn thumbnail_slots_for_viewport(viewport_height: f32) -> usize {
    let available_height =
        (viewport_height - THUMBNAIL_TOP_PADDING - THUMBNAIL_DRAW_TOP_INSET).max(0.0);
    (available_height / THUMBNAIL_ITEM_HEIGHT).floor().max(1.0) as usize
}

fn zoomed_fit_viewport(size: Size, width: f32, height: f32, zoom: f32) -> Viewport {
    let content = SlideRect::new(Point::ZERO, size.width, size.height);
    let bounds = SlideRect::new(Point::ZERO, width, height);
    let fit = Viewport::fit(content, bounds);
    let fit_scale = fit.transform.scale.max(0.0001);
    let requested = if zoom.is_finite() {
        zoom.clamp(0.25, 4.0)
    } else {
        1.0
    };
    let final_scale = (fit_scale * requested).max(0.0001);
    let translation = Point::new(
        bounds.center().x / final_scale - content.center().x,
        bounds.center().y / final_scale - content.center().y,
    );
    Viewport::new(slides_api::Transform::new(translation, final_scale))
}

struct CachedTexture {
    key: u64,
    texture: Texture,
    width: usize,
    height: usize,
    downsampled: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct TextDrawOutcome {
    drew: bool,
    truncated_by_cap: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImageDrawOutcome {
    Drawn,
    Downsampled,
    Placeholder,
}

#[derive(Script, ScriptHook, Widget)]
pub struct SlidesVisualCanvas {
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
    draw_panel: DrawColor,
    #[live]
    draw_page: DrawColor,
    #[live]
    draw_shadow: DrawColor,
    #[live]
    draw_border: DrawColor,
    #[live]
    draw_shape: DrawColor,
    #[live]
    draw_text: DrawText,
    #[live]
    draw_text_bold: DrawText,
    #[live]
    draw_text_italic: DrawText,
    #[live]
    draw_text_bold_italic: DrawText,
    #[live]
    draw_image: DrawImage,

    #[action_data]
    #[rust]
    action_data: WidgetActionData,
    #[rust]
    frame: Option<SlidesViewerFrame>,
    #[rust]
    textures: Vec<CachedTexture>,
    #[rust]
    last_canvas_rect: Option<Rect>,
    #[rust]
    last_draw_report: Option<SlidesDrawReport>,
    #[rust]
    open_trace_draw_reported: bool,
    #[rust]
    open_trace_deferred_reported: bool,
}

impl SlidesVisualCanvas {
    pub fn set_frame(&mut self, cx: &mut Cx, frame: Option<SlidesViewerFrame>) {
        if frame.is_none() {
            self.open_trace_draw_reported = false;
            self.open_trace_deferred_reported = false;
        }
        self.frame = frame;
        self.last_canvas_rect = None;
        self.last_draw_report = None;
        self.draw_bg.redraw(cx);
    }

    pub fn viewport_points(&self, cx: &Cx) -> (f32, f32) {
        let rect = self.draw_bg.area().rect(cx);
        (rect.size.x as f32, rect.size.y as f32)
    }

    pub fn needs_viewport_reframe(&self, cx: &Cx) -> bool {
        let (width, height) = self.viewport_points(cx);
        self.frame
            .as_ref()
            .is_some_and(|frame| frame.needs_viewport_reframe(width, height))
    }

    /// Returns bounded semantic evidence for the central slide currently sent
    /// through this canvas. It is intentionally derived from the same frame as
    /// drawing, never from a second projection path.
    pub fn accessibility_snapshot(&self) -> Option<SlidesVisualAccessibilitySnapshot> {
        let frame = self.frame.as_ref()?;
        let plan = frame.central.as_ref()?;
        // Accessibility is refreshed after Draw. Reusing the rect recorded by the
        // actual draw pass avoids querying an uninitialized Makepad area during
        // document-open transitions.
        let canvas = self.last_canvas_rect?;
        let central = Self::central_canvas_rect(canvas, frame);
        let report = self
            .last_draw_report
            .unwrap_or_else(|| SlidesDrawReport::for_plan(plan));
        Some(SlidesVisualAccessibilitySnapshot {
            summary: render_accessibility_summary(report),
            draw_called: self.last_draw_report.is_some(),
            items: semantic_items_for_plan(plan, central.pos, central),
        })
    }

    fn central_canvas_rect(canvas: Rect, frame: &SlidesViewerFrame) -> Rect {
        if frame.presentation_mode {
            canvas
        } else {
            Rect {
                pos: dvec2(canvas.pos.x + THUMBNAIL_PANE_WIDTH as f64, canvas.pos.y),
                size: dvec2(
                    (canvas.size.x - THUMBNAIL_PANE_WIDTH as f64).max(1.0),
                    canvas.size.y,
                ),
            }
        }
    }

    fn emit(&self, cx: &mut Cx, action: SlidesVisualAction) {
        cx.widget_action_with_data(&self.action_data, self.uid, action);
    }

    fn thumbnail_at(&self, frame: &SlidesViewerFrame, point: DVec2, canvas: Rect) -> Option<usize> {
        if frame.presentation_mode
            || point.x < canvas.pos.x
            || point.x > canvas.pos.x + THUMBNAIL_PANE_WIDTH as f64
        {
            return None;
        }
        let local_y = point.y - canvas.pos.y - THUMBNAIL_TOP_PADDING as f64;
        if local_y < 0.0 {
            return None;
        }
        let slot = (local_y / THUMBNAIL_ITEM_HEIGHT as f64).floor() as usize;
        frame
            .thumbnails
            .get(slot)
            .filter(|_| local_y <= (slot + 1) as f64 * THUMBNAIL_ITEM_HEIGHT as f64)
            .map(|item| item.index)
    }

    fn text_object_at(
        &self,
        frame: &SlidesViewerFrame,
        point: DVec2,
        canvas: Rect,
    ) -> Option<ObjectId> {
        let central = Self::central_canvas_rect(canvas, frame);
        if point.x < central.pos.x
            || point.y < central.pos.y
            || point.x > central.pos.x + central.size.x
            || point.y > central.pos.y + central.size.y
        {
            return None;
        }
        let local = Point::new(
            (point.x - central.pos.x) as f32,
            (point.y - central.pos.y) as f32,
        );
        frame
            .text_hit_targets
            .iter()
            .rev()
            .find(|target| target.rect.contains(local))
            .map(|target| target.object_id)
    }

    fn draw_rect(&mut self, cx: &mut Cx2d, rect: Rect, color: Vec4f) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        self.draw_shape.color = color;
        self.draw_shape.draw_abs(cx, rect);
    }

    fn local_rect(origin: DVec2, rect: SlideRect) -> Rect {
        Rect {
            pos: dvec2(
                origin.x + rect.origin.x as f64,
                origin.y + rect.origin.y as f64,
            ),
            size: dvec2(rect.width as f64, rect.height as f64),
        }
    }

    fn page_rect(plan: &RenderPlan) -> Option<SlideRect> {
        let transform = plan.viewport.transform;
        let left = (transform.translation.x * transform.scale)
            .min((plan.slide_size.width + transform.translation.x) * transform.scale);
        let top = (transform.translation.y * transform.scale)
            .min((plan.slide_size.height + transform.translation.y) * transform.scale);
        let right = (transform.translation.x + plan.slide_size.width + 0.0) * transform.scale;
        let bottom = (transform.translation.y + plan.slide_size.height + 0.0) * transform.scale;
        let rect = SlideRect::new(
            Point::new(left, top),
            (right - left).abs(),
            (bottom - top).abs(),
        );
        rect.is_valid().then_some(rect)
    }

    fn draw_plan(
        &mut self,
        cx: &mut Cx2d,
        plan: &RenderPlan,
        origin: DVec2,
        page: SlideRect,
    ) -> SlidesDrawReport {
        let mut report = SlidesDrawReport::for_plan(plan);
        let mut clip_depth = 0usize;
        for command in plan.commands.iter().take(MAX_RENDER_COMMANDS) {
            match command {
                RenderCommand::Clear { color } => {
                    self.draw_page.color = color_from_array(*color);
                    self.draw_page.draw_abs(cx, Self::local_rect(origin, page));
                    report.drawn_commands += 1;
                }
                RenderCommand::FillRect { rect, color }
                | RenderCommand::FillEllipse { rect, color } => {
                    let target = Self::local_rect(origin, *rect);
                    if matches!(command, RenderCommand::FillEllipse { .. }) {
                        self.draw_ellipse(cx, target, color_from_array(*color), 0.0);
                    } else {
                        self.draw_rect(cx, target, color_from_array(*color));
                    }
                    report.drawn_commands += 1;
                }
                RenderCommand::StrokeRect { rect, color, width } => {
                    self.draw_outline(
                        cx,
                        Self::local_rect(origin, *rect),
                        color_from_array(*color),
                        *width,
                    );
                    report.drawn_commands += 1;
                }
                RenderCommand::StrokeEllipse { rect, color, width } => {
                    self.draw_ellipse(
                        cx,
                        Self::local_rect(origin, *rect),
                        color_from_array(*color),
                        *width,
                    );
                    report.drawn_commands += 1;
                }
                RenderCommand::FillChevron { rect, color } => {
                    self.draw_chevron(
                        cx,
                        Self::local_rect(origin, *rect),
                        color_from_array(*color),
                    );
                    report.drawn_commands += 1;
                }
                RenderCommand::StrokeChevron { rect, color, width } => {
                    self.draw_chevron_outline(
                        cx,
                        Self::local_rect(origin, *rect),
                        color_from_array(*color),
                        *width,
                    );
                    report.drawn_commands += 1;
                }
                RenderCommand::DrawLine {
                    start,
                    end,
                    color,
                    width,
                } => {
                    self.draw_line(cx, origin, *start, *end, color_from_array(*color), *width);
                    report.drawn_commands += 1;
                }
                RenderCommand::DrawText { rect, text, color } => {
                    let outcome = self.draw_text_object(
                        cx,
                        origin,
                        *rect,
                        text,
                        color_from_array(*color),
                        plan.viewport.transform.scale,
                    );
                    if outcome.drew {
                        report.drawn_commands += 1;
                    }
                    if outcome.truncated_by_cap {
                        report.text_cap_truncations += 1;
                    }
                }
                RenderCommand::DrawImage {
                    rect,
                    format,
                    bytes,
                    ..
                } => {
                    match self.draw_image_object(cx, origin, *rect, *format, bytes) {
                        ImageDrawOutcome::Drawn => {}
                        ImageDrawOutcome::Downsampled => report.image_downsamples += 1,
                        ImageDrawOutcome::Placeholder => report.image_placeholders += 1,
                    }
                    report.drawn_commands += 1;
                }
                RenderCommand::PushClip { rect } => {
                    let _ = cx.push_clip_rect_tracked(Self::local_rect(origin, *rect));
                    clip_depth += 1;
                    report.applied_clip_commands += 1;
                }
                RenderCommand::PopClip => {
                    if clip_depth > 0 {
                        cx.pop_clip_rect();
                        clip_depth -= 1;
                        report.applied_clip_commands += 1;
                    }
                }
            }
        }
        // The bounded command limit may stop in the middle of an otherwise
        // balanced plan. Never let a transient plan clip leak into later
        // canvas or thumbnail drawing.
        while clip_depth > 0 {
            cx.pop_clip_rect();
            clip_depth -= 1;
            report.auto_closed_clip_commands += 1;
        }
        report
    }

    fn draw_outline(&mut self, cx: &mut Cx2d, rect: Rect, color: Vec4f, width: f32) {
        let thickness = (width.max(1.0) as f64).min(rect.size.x.min(rect.size.y).max(1.0));
        self.draw_rect(
            cx,
            Rect {
                pos: rect.pos,
                size: dvec2(rect.size.x, thickness),
            },
            color,
        );
        self.draw_rect(
            cx,
            Rect {
                pos: dvec2(rect.pos.x, rect.pos.y + rect.size.y - thickness),
                size: dvec2(rect.size.x, thickness),
            },
            color,
        );
        self.draw_rect(
            cx,
            Rect {
                pos: rect.pos,
                size: dvec2(thickness, rect.size.y),
            },
            color,
        );
        self.draw_rect(
            cx,
            Rect {
                pos: dvec2(rect.pos.x + rect.size.x - thickness, rect.pos.y),
                size: dvec2(thickness, rect.size.y),
            },
            color,
        );
    }

    fn draw_ellipse(&mut self, cx: &mut Cx2d, rect: Rect, color: Vec4f, stroke: f32) {
        let rows = 18usize;
        let rx = rect.size.x * 0.5;
        let ry = rect.size.y * 0.5;
        let center_x = rect.pos.x + rx;
        let center_y = rect.pos.y + ry;
        for row in 0..rows {
            let t = (row as f64 + 0.5) / rows as f64 * 2.0 - 1.0;
            let inside = (1.0 - t * t).max(0.0).sqrt() * rx;
            let y = center_y + t * ry;
            if stroke <= 0.0 {
                self.draw_rect(
                    cx,
                    Rect {
                        pos: dvec2(center_x - inside, y - ry / rows as f64),
                        size: dvec2(inside * 2.0, ry * 2.0 / rows as f64 + 0.5),
                    },
                    color,
                );
            } else {
                let inner = (inside - stroke as f64).max(0.0);
                let thickness = (stroke as f64).min(inside.max(1.0));
                self.draw_rect(
                    cx,
                    Rect {
                        pos: dvec2(center_x - inside, y - ry / rows as f64),
                        size: dvec2(thickness, ry * 2.0 / rows as f64 + 0.5),
                    },
                    color,
                );
                self.draw_rect(
                    cx,
                    Rect {
                        pos: dvec2(center_x + inner, y - ry / rows as f64),
                        size: dvec2(thickness, ry * 2.0 / rows as f64 + 0.5),
                    },
                    color,
                );
            }
        }
    }

    fn draw_chevron(&mut self, cx: &mut Cx2d, rect: Rect, color: Vec4f) {
        if rect.size.x <= 0.0 || rect.size.y <= 0.0 {
            return;
        }
        let rows = (rect.size.y.ceil() as usize).clamp(1, 96);
        let band_height = rect.size.y / rows as f64;
        let inset = rect.size.x * 0.25;
        for row in 0..rows {
            let t = (row as f64 + 0.5) / rows as f64;
            let center_weight = (1.0 - (t * 2.0 - 1.0).abs()).clamp(0.0, 1.0);
            let left = rect.pos.x + inset * center_weight;
            let right = rect.pos.x + rect.size.x - inset + inset * center_weight;
            self.draw_rect(
                cx,
                Rect {
                    pos: dvec2(left, rect.pos.y + row as f64 * band_height),
                    size: dvec2((right - left).max(0.0), band_height + 0.5),
                },
                color,
            );
        }
    }

    fn draw_chevron_outline(&mut self, cx: &mut Cx2d, rect: Rect, color: Vec4f, width: f32) {
        let points = chevron_points(rect);
        for index in 0..points.len() {
            self.draw_segment(
                cx,
                points[index],
                points[(index + 1) % points.len()],
                color,
                width,
            );
        }
    }

    fn draw_segment(&mut self, cx: &mut Cx2d, start: DVec2, end: DVec2, color: Vec4f, width: f32) {
        let thickness = f64::from(width.max(1.0));
        let distance = (end - start).length();
        let steps = (distance / (thickness * 0.75)).ceil().clamp(1.0, 128.0) as usize;
        for step in 0..=steps {
            let t = step as f64 / steps as f64;
            let point = start + (end - start) * t;
            self.draw_rect(
                cx,
                Rect {
                    pos: dvec2(point.x - thickness * 0.5, point.y - thickness * 0.5),
                    size: dvec2(thickness, thickness),
                },
                color,
            );
        }
    }

    fn draw_line(
        &mut self,
        cx: &mut Cx2d,
        origin: DVec2,
        start: Point,
        end: Point,
        color: Vec4f,
        width: f32,
    ) {
        let steps = 24usize;
        let size = width.max(1.0) as f64;
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            let x = start.x + (end.x - start.x) * t;
            let y = start.y + (end.y - start.y) * t;
            self.draw_rect(
                cx,
                Rect {
                    pos: dvec2(
                        origin.x + x as f64 - size * 0.5,
                        origin.y + y as f64 - size * 0.5,
                    ),
                    size: dvec2(size, size),
                },
                color,
            );
        }
    }

    fn draw_text_object(
        &mut self,
        cx: &mut Cx2d,
        origin: DVec2,
        rect: SlideRect,
        text: &Text,
        fallback: Vec4f,
        viewport_scale: f32,
    ) -> TextDrawOutcome {
        let total_chars: usize = text
            .paragraphs
            .iter()
            .map(|p| p.runs.iter().map(|r| r.text.chars().count()).sum::<usize>())
            .sum();
        let max_chars = MAX_TEXT_CHARS.min(total_chars);
        if max_chars == 0 {
            return TextDrawOutcome::default();
        }
        let mut outcome = TextDrawOutcome {
            drew: false,
            truncated_by_cap: total_chars > max_chars,
        };
        let mut y = rect.origin.y as f64;
        let mut consumed = 0usize;
        for paragraph in &text.paragraphs {
            let font_size = paragraph_font_size(paragraph, viewport_scale);
            let line_height = (font_size as f64 * 1.25).max(8.0);
            let line = paragraph_text(paragraph, max_chars.saturating_sub(consumed));
            if line.is_empty() {
                y += line_height;
                continue;
            }
            let estimated = line.chars().count() as f64 * font_size as f64 * 0.52;
            let x = match paragraph.formatting.alignment {
                TextAlignment::Center => {
                    rect.origin.x as f64 + (rect.width as f64 - estimated).max(0.0) * 0.5
                }
                TextAlignment::Right => {
                    rect.origin.x as f64 + (rect.width as f64 - estimated).max(0.0)
                }
                TextAlignment::Left | TextAlignment::Justify => {
                    rect.origin.x as f64 + paragraph.formatting.indent.max(0.0) as f64
                }
            };
            self.draw_runs(
                cx,
                origin,
                x,
                y + font_size as f64,
                rect,
                paragraph,
                fallback,
                viewport_scale,
            );
            outcome.drew = true;
            y += line_height;
            consumed = consumed.saturating_add(line.chars().count());
            if y > rect.origin.y as f64 + rect.height as f64 {
                break;
            }
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_runs(
        &mut self,
        cx: &mut Cx2d,
        origin: DVec2,
        mut x: f64,
        baseline: f64,
        rect: SlideRect,
        paragraph: &Paragraph,
        fallback: Vec4f,
        viewport_scale: f32,
    ) {
        for run in &paragraph.runs {
            if run.text.is_empty() {
                continue;
            }
            let formatting = &run.formatting;
            let size = (formatting.font_size * viewport_scale).clamp(6.0, 96.0);
            let available = ((rect.origin.x + rect.width) as f64 - x).max(0.0);
            let max_chars = ((available / (size as f64 * 0.52).max(1.0)).floor() as usize).min(256);
            if max_chars == 0 {
                break;
            }
            let clipped: String = run.text.chars().take(max_chars).collect();
            let color = color_from_api(formatting.color);
            let draw = match (formatting.bold, formatting.italic) {
                (true, true) => &mut self.draw_text_bold_italic,
                (true, false) => &mut self.draw_text_bold,
                (false, true) => &mut self.draw_text_italic,
                (false, false) => &mut self.draw_text,
            };
            draw.color = if formatting.color == Color::default() {
                fallback
            } else {
                color
            };
            draw.text_style.font_size = size;
            draw.draw_abs(cx, dvec2(origin.x + x, origin.y + baseline), &clipped);
            x += clipped.chars().count() as f64 * size as f64 * 0.52;
        }
    }

    fn draw_image_object(
        &mut self,
        cx: &mut Cx2d,
        origin: DVec2,
        rect: SlideRect,
        format: ImageFormat,
        bytes: &[u8],
    ) -> ImageDrawOutcome {
        if !image_source_is_admissible(format, bytes) {
            self.draw_placeholder(cx, Self::local_rect(origin, rect));
            return ImageDrawOutcome::Placeholder;
        }
        let key = image_key(format, bytes);
        let (texture, width, height, downsampled) = if let Some(cached) =
            self.textures.iter().find(|item| item.key == key)
        {
            (
                cached.texture.clone(),
                cached.width,
                cached.height,
                cached.downsampled,
            )
        } else {
            let Some(image) = decode_slide_image(format, bytes) else {
                self.draw_placeholder(cx, Self::local_rect(origin, rect));
                return ImageDrawOutcome::Placeholder;
            };
            let (image, downsampled) = downsample_image_to_pixel_budget(image, MAX_TEXTURE_PIXELS);
            let width = image.width;
            let height = image.height;
            let texture = image.into_new_texture(cx);
            if self.textures.len() >= MAX_TEXTURES {
                self.textures.remove(0);
            }
            self.textures.push(CachedTexture {
                key,
                texture: texture.clone(),
                width,
                height,
                downsampled,
            });
            (texture, width, height, downsampled)
        };
        self.draw_image.draw_vars.set_texture(0, &texture);
        self.draw_image.image_dim_w = width as f32;
        self.draw_image.image_dim_h = height as f32;
        self.draw_image.draw_abs(cx, Self::local_rect(origin, rect));
        if downsampled {
            ImageDrawOutcome::Downsampled
        } else {
            ImageDrawOutcome::Drawn
        }
    }

    fn draw_placeholder(&mut self, cx: &mut Cx2d, rect: Rect) {
        self.draw_rect(cx, rect, Vec4f::from_u32(0xe7ebf0ff));
        self.draw_outline(cx, rect, Vec4f::from_u32(0x9aa8b8ff), 1.0);
    }

    fn draw_central(&mut self, cx: &mut Cx2d, canvas: Rect, plan: &RenderPlan) -> SlidesDrawReport {
        let Some(page) = Self::page_rect(plan) else {
            return SlidesDrawReport::for_plan(plan);
        };
        let page_rect = Self::local_rect(canvas.pos, page);
        self.draw_shadow.color = Vec4f::from_u32(0x00000042);
        self.draw_shadow.draw_abs(
            cx,
            Rect {
                pos: dvec2(page_rect.pos.x + 6.0, page_rect.pos.y + 7.0),
                size: page_rect.size,
            },
        );
        let report = self.draw_plan(cx, plan, canvas.pos, page);
        self.draw_outline(cx, page_rect, Vec4f::from_u32(0x8e9aaaff), 1.0);
        report
    }

    fn draw_thumbnails(&mut self, cx: &mut Cx2d, canvas: Rect, frame: &SlidesViewerFrame) {
        self.draw_panel.color = Vec4f::from_u32(0x1b2738ff);
        self.draw_panel.draw_abs(
            cx,
            Rect {
                pos: canvas.pos,
                size: dvec2(THUMBNAIL_PANE_WIDTH as f64, canvas.size.y),
            },
        );
        for (slot, item) in frame.thumbnails.iter().enumerate() {
            let x = canvas.pos.x + 16.0;
            let y = canvas.pos.y
                + THUMBNAIL_TOP_PADDING as f64
                + slot as f64 * THUMBNAIL_ITEM_HEIGHT as f64
                + THUMBNAIL_DRAW_TOP_INSET as f64;
            let thumb = Rect {
                pos: dvec2(x, y),
                size: dvec2(THUMBNAIL_BOX_WIDTH as f64, THUMBNAIL_BOX_HEIGHT as f64),
            };
            if item.selected {
                self.draw_border.color = Vec4f::from_u32(0x4e9bffff);
                self.draw_border.draw_abs(
                    cx,
                    Rect {
                        pos: dvec2(thumb.pos.x - 3.0, thumb.pos.y - 3.0),
                        size: dvec2(thumb.size.x + 6.0, thumb.size.y + 6.0),
                    },
                );
            }
            if let Some(page) = Self::page_rect(&item.plan) {
                let _ = self.draw_plan(cx, &item.plan, thumb.pos, page);
            }
            self.draw_text_at(
                cx,
                &format!("{}", item.index + 1),
                canvas.pos.x + 8.0,
                y + THUMBNAIL_BOX_HEIGHT as f64 + 18.0,
                Vec4f::from_u32(0xc4cfddff),
                11.0,
                item.selected,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text_at(
        &mut self,
        cx: &mut Cx2d,
        text: &str,
        x: f64,
        baseline: f64,
        color: Vec4f,
        size: f32,
        bold: bool,
    ) {
        let draw = if bold {
            &mut self.draw_text_bold
        } else {
            &mut self.draw_text
        };
        draw.color = color;
        draw.text_style.font_size = size;
        draw.draw_abs(cx, dvec2(x, baseline), text);
    }
}

impl Widget for SlidesVisualCanvas {
    #[allow(clippy::collapsible_if)]
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, _scope: &mut Scope) {
        let area = self.draw_bg.area();
        match event.hits_with_capture_overload(cx, area, true) {
            Hit::FingerDown(fe) if fe.is_primary_hit() => {
                cx.set_key_focus(area);
                if let Some(frame) = self.frame.as_ref()
                    && let Some(index) = self.thumbnail_at(frame, fe.abs, area.rect(cx))
                {
                    self.emit(cx, SlidesVisualAction::ThumbnailClicked(index));
                    return;
                }
                if let Some(frame) = self.frame.as_ref() {
                    let object_id = self.text_object_at(frame, fe.abs, area.rect(cx));
                    self.emit(cx, SlidesVisualAction::TextObjectClicked(object_id));
                }
            }
            Hit::FingerScroll(fe) => self.emit(
                cx,
                SlidesVisualAction::Wheel {
                    delta_x: fe.scroll.x.round() as i64,
                    delta_y: fe.scroll.y.round() as i64,
                    control: fe.modifiers.control,
                },
            ),
            Hit::KeyDown(ke) => self.emit(cx, SlidesVisualAction::KeyDown(ke.key_code)),
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let canvas = cx.walk_turtle(walk);
        self.last_canvas_rect = Some(canvas);
        self.draw_bg.color = Vec4f::from_u32(0x0d1624ff);
        self.draw_bg.draw_abs(cx, canvas);
        let Some(frame) = self.frame.clone() else {
            return DrawStep::done();
        };
        if !frame.presentation_mode {
            self.draw_thumbnails(cx, canvas, &frame);
        }
        if let Some(plan) = frame.central.as_ref() {
            let central_canvas = Self::central_canvas_rect(canvas, &frame);
            let report = self.draw_central(cx, central_canvas, plan);
            if let Some(object_id) = frame.selected_text_object
                && let Some(target) = frame
                    .text_hit_targets
                    .iter()
                    .find(|target| target.object_id == object_id)
            {
                self.draw_outline(
                    cx,
                    Self::local_rect(central_canvas.pos, target.rect),
                    Vec4f::from_u32(0x2563ebff),
                    2.0,
                );
            }
            if !self.open_trace_draw_reported
                && std::env::var_os("RUDRA_DOCUMENT_OPEN_TRACE").is_some()
            {
                eprintln!(
                    "RUDRA_OPEN stage=first_makepad_draw status=PASS format=pptx canvas={:.1}x{:.1} slides={} plan_commands={} considered={} draw_commands={} text_commands={} image_commands={} graphics={} lines={} warnings={} clips={} unbalanced_clip={} applied_clip={} auto_closed_clip={} image_downsamples={} image_placeholders={} text_cap_truncations={}",
                    canvas.size.x,
                    canvas.size.y,
                    frame.slide_count,
                    report.plan.total_commands,
                    report.plan.considered_commands,
                    report.drawn_commands,
                    report.plan.text_commands,
                    report.plan.image_commands,
                    report.plan.graphic_commands,
                    report.plan.line_commands,
                    report.plan.warning_count,
                    report.plan.clip_commands,
                    report.plan.unbalanced_clip_commands,
                    report.applied_clip_commands,
                    report.auto_closed_clip_commands,
                    report.image_downsamples,
                    report.image_placeholders,
                    report.text_cap_truncations,
                );
                self.open_trace_draw_reported = true;
            }
            self.last_draw_report = Some(report);
        } else {
            self.last_draw_report = None;
            if !self.open_trace_deferred_reported
                && std::env::var_os("RUDRA_DOCUMENT_OPEN_TRACE").is_some()
            {
                eprintln!(
                    "RUDRA_OPEN stage=first_makepad_draw status=DEFERRED format=pptx canvas={:.1}x{:.1} slides={} reason=viewport_not_ready",
                    canvas.size.x, canvas.size.y, frame.slide_count,
                );
                self.open_trace_deferred_reported = true;
            }
        }
        DrawStep::done()
    }
}

fn render_accessibility_summary(report: SlidesDrawReport) -> String {
    let mut detail = Vec::new();
    if report.plan.warning_count > 0 {
        detail.push(format!("предупреждения {}", report.plan.warning_count));
    }
    if report.plan.command_cap_reached {
        detail.push("достигнут лимит команд".to_owned());
    }
    if report.plan.unbalanced_clip_commands > 0 {
        detail.push(format!(
            "некорректный clip {}",
            report.plan.unbalanced_clip_commands
        ));
    }
    if report.auto_closed_clip_commands > 0 {
        detail.push(format!(
            "clip закрыт по лимиту {}",
            report.auto_closed_clip_commands
        ));
    }
    if report.image_downsamples > 0 {
        detail.push(format!(
            "изображения уменьшены {}",
            report.image_downsamples
        ));
    }
    if report.image_placeholders > 0 {
        detail.push(format!(
            "placeholder изображений {}",
            report.image_placeholders
        ));
    }
    if report.text_cap_truncations > 0 {
        detail.push(format!(
            "ограничение текста {}",
            report.text_cap_truncations
        ));
    }
    if report.needs_disclosure() {
        detail.insert(0, "частичное отображение".to_owned());
    }
    let suffix = if detail.is_empty() {
        String::new()
    } else {
        format!("; {}", detail.join(", "))
    };
    format!(
        "Рендер слайда: plan {}, нарисовано {}, текст {}, изображения {}, графика {}, линии {}{}",
        report.plan.total_commands,
        report.drawn_commands,
        report.plan.text_commands,
        report.plan.image_commands,
        report.plan.graphic_commands,
        report.plan.line_commands,
        suffix,
    )
}

fn semantic_items_for_plan(
    plan: &RenderPlan,
    origin: DVec2,
    visible_bounds: Rect,
) -> Vec<SlidesAccessibilityItem> {
    plan.commands
        .iter()
        .take(MAX_RENDER_COMMANDS)
        .enumerate()
        .filter_map(|(command_index, command)| {
            let (kind, name, rect) = match command {
                RenderCommand::DrawText { rect, text, .. } => (
                    SlidesAccessibilityKind::Text,
                    format!("Текст: {}", semantic_text_preview(text)),
                    SlidesVisualCanvas::local_rect(origin, *rect),
                ),
                RenderCommand::DrawImage {
                    rect,
                    format,
                    alt_text,
                    ..
                } => (
                    SlidesAccessibilityKind::Image,
                    semantic_image_name(*format, alt_text.as_deref()),
                    SlidesVisualCanvas::local_rect(origin, *rect),
                ),
                RenderCommand::FillRect { rect, .. }
                | RenderCommand::StrokeRect { rect, .. }
                | RenderCommand::FillEllipse { rect, .. }
                | RenderCommand::StrokeEllipse { rect, .. }
                | RenderCommand::FillChevron { rect, .. }
                | RenderCommand::StrokeChevron { rect, .. } => (
                    SlidesAccessibilityKind::Graphic,
                    "Графический объект".to_owned(),
                    SlidesVisualCanvas::local_rect(origin, *rect),
                ),
                RenderCommand::DrawLine {
                    start, end, width, ..
                } => {
                    let thickness = f64::from(width.max(1.0));
                    let left = f64::from(start.x.min(end.x)) - thickness * 0.5;
                    let top = f64::from(start.y.min(end.y)) - thickness * 0.5;
                    let right = f64::from(start.x.max(end.x)) + thickness * 0.5;
                    let bottom = f64::from(start.y.max(end.y)) + thickness * 0.5;
                    (
                        SlidesAccessibilityKind::Graphic,
                        "Линия".to_owned(),
                        Rect {
                            pos: dvec2(origin.x + left, origin.y + top),
                            size: dvec2((right - left).max(1.0), (bottom - top).max(1.0)),
                        },
                    )
                }
                RenderCommand::Clear { .. }
                | RenderCommand::PushClip { .. }
                | RenderCommand::PopClip => return None,
            };
            rect_intersection(rect, visible_bounds).map(|bounds| SlidesAccessibilityItem {
                command_index,
                kind,
                name,
                bounds,
            })
        })
        .take(MAX_SEMANTIC_ITEMS)
        .collect()
}

fn chevron_points(rect: Rect) -> [DVec2; 6] {
    let inset = rect.size.x * 0.25;
    [
        rect.pos,
        dvec2(rect.pos.x + rect.size.x - inset, rect.pos.y),
        dvec2(rect.pos.x + rect.size.x, rect.pos.y + rect.size.y * 0.5),
        dvec2(rect.pos.x + rect.size.x - inset, rect.pos.y + rect.size.y),
        dvec2(rect.pos.x, rect.pos.y + rect.size.y),
        dvec2(rect.pos.x + inset, rect.pos.y + rect.size.y * 0.5),
    ]
}
fn rect_intersection(first: Rect, second: Rect) -> Option<Rect> {
    let left = first.pos.x.max(second.pos.x);
    let top = first.pos.y.max(second.pos.y);
    let right = (first.pos.x + first.size.x).min(second.pos.x + second.size.x);
    let bottom = (first.pos.y + first.size.y).min(second.pos.y + second.size.y);

    (right > left && bottom > top).then_some(Rect {
        pos: dvec2(left, top),
        size: dvec2(right - left, bottom - top),
    })
}

fn semantic_text_preview(text: &Text) -> String {
    let mut preview = String::new();
    let mut pending_space = false;
    let mut truncated = false;
    for character in text
        .paragraphs
        .iter()
        .flat_map(|paragraph| paragraph.runs.iter())
        .flat_map(|run| run.text.chars())
    {
        if character.is_whitespace() {
            pending_space = !preview.is_empty();
            continue;
        }
        if pending_space {
            preview.push(' ');
            pending_space = false;
        }
        if preview.chars().count() >= SEMANTIC_TEXT_CHAR_CAP {
            truncated = true;
            break;
        }
        preview.push(character);
    }
    if preview.is_empty() {
        preview.push_str("[пустой текст]");
    }
    if truncated {
        preview.push('…');
    }
    preview
}

fn semantic_image_name(format: ImageFormat, alt_text: Option<&str>) -> String {
    let detail = alt_text
        .filter(|value| !value.trim().is_empty())
        .map(str::trim)
        .unwrap_or_else(|| match format {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
            ImageFormat::Bmp => "BMP",
            ImageFormat::Svg => "SVG",
            ImageFormat::Unknown => "неизвестный формат",
        });
    format!("Изображение: {detail}")
}

fn paragraph_text(paragraph: &Paragraph, limit: usize) -> String {
    paragraph
        .runs
        .iter()
        .flat_map(|run| run.text.chars())
        .take(limit)
        .collect()
}

fn paragraph_font_size(paragraph: &Paragraph, scale: f32) -> f32 {
    paragraph
        .runs
        .iter()
        .map(|run| run.formatting.font_size)
        .filter(|size| size.is_finite() && *size > 0.0)
        .fold(12.0, f32::max)
        .mul_add(scale, 0.0)
        .clamp(6.0, 96.0)
}

fn image_key(format: ImageFormat, bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    format.hash(&mut hasher);
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn decode_slide_image(format: ImageFormat, bytes: &[u8]) -> Option<ImageBuffer> {
    match format {
        ImageFormat::Png => ImageBuffer::from_png(bytes).ok(),
        ImageFormat::Jpeg => ImageBuffer::from_jpg(bytes).ok(),
        ImageFormat::Bmp => ImageBuffer::from_bmp(bytes).ok(),
        ImageFormat::Svg | ImageFormat::Unknown => None,
    }
}

fn image_source_is_admissible(format: ImageFormat, bytes: &[u8]) -> bool {
    if !image_byte_count_is_admissible(bytes.len()) {
        return false;
    }
    source_image_dimensions(format, bytes)
        .and_then(|(width, height)| width.checked_mul(height))
        .is_some_and(|pixels| pixels <= MAX_IMAGE_SOURCE_PIXELS)
}

fn image_byte_count_is_admissible(byte_count: usize) -> bool {
    byte_count > 0 && byte_count <= MAX_IMAGE_BYTES
}

fn source_image_dimensions(format: ImageFormat, bytes: &[u8]) -> Option<(usize, usize)> {
    match format {
        ImageFormat::Png => png_dimensions(bytes),
        ImageFormat::Jpeg => jpeg_dimensions(bytes),
        ImageFormat::Bmp => bmp_dimensions(bytes),
        ImageFormat::Svg | ImageFormat::Unknown => None,
    }
}

fn png_dimensions(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.len() < 24 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?) as usize;
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?) as usize;
    (width > 0 && height > 0).then_some((width, height))
}

fn bmp_dimensions(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.len() < 26 || !bytes.starts_with(b"BM") {
        return None;
    }
    let dib_size = u32::from_le_bytes(bytes[14..18].try_into().ok()?);
    if dib_size < 40 {
        return None;
    }
    let width = i32::from_le_bytes(bytes[18..22].try_into().ok()?);
    let height = i32::from_le_bytes(bytes[22..26].try_into().ok()?);
    let height = height.checked_abs()? as usize;
    (width > 0 && height > 0).then_some((width as usize, height))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(usize, usize)> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut index = 2usize;
    while index < bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }
        while index < bytes.len() && bytes[index] == 0xff {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let marker = bytes[index];
        index += 1;
        match marker {
            0xd8 | 0xd9 | 0x01 | 0xd0..=0xd7 => continue,
            0xda => break,
            _ => {}
        }
        if index + 1 >= bytes.len() {
            return None;
        }
        let length = u16::from_be_bytes([bytes[index], bytes[index + 1]]) as usize;
        if length < 2 || index + length > bytes.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if length < 8 {
                return None;
            }
            let height = u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]) as usize;
            let width = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as usize;
            return (width > 0 && height > 0).then_some((width, height));
        }
        index += length;
    }
    None
}

fn downsample_image_to_pixel_budget(
    mut image: ImageBuffer,
    max_pixels: usize,
) -> (ImageBuffer, bool) {
    let max_pixels = max_pixels.max(1);
    let source_pixels = image.width.saturating_mul(image.height);
    if source_pixels <= max_pixels {
        return (image, false);
    }
    let scale = (max_pixels as f64 / source_pixels as f64).sqrt();
    let mut width = ((image.width as f64 * scale).floor() as usize).clamp(1, image.width);
    let mut height = ((image.height as f64 * scale).floor() as usize).clamp(1, image.height);
    while width.saturating_mul(height) > max_pixels {
        if width >= height && width > 1 {
            width -= 1;
        } else if height > 1 {
            height -= 1;
        } else {
            break;
        }
    }
    let source_width = image.width;
    let source_height = image.height;
    let mut data = Vec::with_capacity(width.saturating_mul(height));
    for y in 0..height {
        let source_y = y.saturating_mul(source_height) / height;
        for x in 0..width {
            let source_x = x.saturating_mul(source_width) / width;
            data.push(image.data[source_y * source_width + source_x]);
        }
    }
    image.width = width;
    image.height = height;
    image.data = data;
    (image, true)
}

fn color_from_array(color: [f32; 4]) -> Vec4f {
    Vec4f {
        x: color[0],
        y: color[1],
        z: color[2],
        w: color[3],
    }
}

fn color_from_api(color: Color) -> Vec4f {
    Vec4f {
        x: color.r as f32 / 255.0,
        y: color.g as f32 / 255.0,
        z: color.b as f32 / 255.0,
        w: color.a as f32 / 255.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slides_api::{Presentation, PresentationId, Slide, SlideId};

    #[test]
    fn empty_presentation_has_empty_frame_without_rendering() {
        let viewer = PresentationViewer::from_document(Presentation::new(PresentationId::new(1)))
            .expect("empty read-only presentation is valid");
        let frame = SlidesViewerFrame::from_viewer(&viewer, 900.0, 600.0, 1.0, false);
        assert_eq!(frame.slide_count, 0);
        assert!(frame.central.is_none());
        assert!(frame.thumbnails.is_empty());
    }

    #[test]
    fn large_presentation_uses_bounded_virtual_thumbnail_window() {
        let presentation = Presentation {
            id: PresentationId::new(1),
            slides: (0..200)
                .map(|index| Slide::new(SlideId::new(index as u64 + 1)))
                .collect(),
        };
        let viewer = PresentationViewer::from_document(presentation).unwrap();
        let frame = SlidesViewerFrame::from_viewer(&viewer, 1200.0, 700.0, 1.0, false);
        assert_eq!(frame.slide_count, 200);
        assert!(frame.thumbnails.len() <= MAX_THUMBNAILS_IN_FRAME);
        assert!(frame.thumbnail_end - frame.thumbnail_start <= MAX_THUMBNAILS_IN_FRAME);
        assert!(frame.thumbnail_start <= frame.current_index);
    }

    #[test]
    fn fit_transform_preserves_custom_and_portrait_aspect_ratios() {
        let wide = zoomed_fit_viewport(Size::new(1920.0, 1080.0), 600.0, 500.0, 1.0);
        let portrait = zoomed_fit_viewport(Size::new(720.0, 1280.0), 600.0, 500.0, 1.0);
        assert!((wide.transform.scale - 0.3125).abs() < 0.0001);
        assert!((portrait.transform.scale - 0.390625).abs() < 0.0001);
        assert!(wide.transform.translation.x.is_finite());
        assert!(portrait.transform.translation.y.is_finite());
    }

    #[test]
    fn deferred_slide_frame_reframes_once_real_viewport_arrives() {
        let pending = SlidesViewerFrame {
            slide_count: 1,
            viewport_width: 1.0,
            viewport_height: 1.0,
            ..Default::default()
        };
        assert!(pending.needs_viewport_reframe(900.0, 600.0));
        let planned = SlidesViewerFrame {
            slide_count: 1,
            viewport_width: 900.0,
            viewport_height: 600.0,
            ..Default::default()
        };
        assert!(!planned.needs_viewport_reframe(900.0, 600.0));
        assert!(planned.needs_viewport_reframe(901.0, 600.0));
    }

    #[test]
    fn frame_exposes_only_text_objects_as_hit_targets_and_keeps_selection() {
        let text_id = ObjectId::new(11);
        let shape_id = ObjectId::new(12);
        let mut slide = Slide::new(SlideId::new(1));
        slide
            .add_object(slides_api::SlideObject::new(
                text_id,
                ObjectKind::Text(Text::plain("Выберите меня")),
                SlideRect::new(Point::new(100.0, 80.0), 320.0, 120.0),
                slides_api::Transform::IDENTITY,
            ))
            .expect("text object");
        slide
            .add_object(slides_api::SlideObject::new(
                shape_id,
                ObjectKind::Shape(slides_api::Shape::default()),
                SlideRect::new(Point::new(500.0, 80.0), 100.0, 100.0),
                slides_api::Transform::IDENTITY,
            ))
            .expect("shape object");
        let mut presentation = Presentation::new(PresentationId::new(1));
        presentation.add_slide(slide).expect("slide");
        let mut viewer = PresentationViewer::from_document(presentation).expect("viewer");
        viewer
            .dispatch(slides_ui::ViewerAction::SelectObject(Some(text_id)))
            .expect("select text");

        let frame = SlidesViewerFrame::from_viewer(&viewer, 1000.0, 700.0, 1.0, false);
        assert_eq!(frame.text_hit_targets.len(), 1);
        assert_eq!(frame.text_hit_targets[0].object_id, text_id);
        assert_eq!(frame.selected_text_object, Some(text_id));
        assert!(frame.text_hit_targets[0].rect.is_valid());
    }

    #[test]
    fn thumbnail_range_is_bounded_and_keeps_current_slide_visible() {
        assert_eq!(
            visible_thumbnail_range_with_window(0, 0, MAX_THUMBNAILS_IN_FRAME),
            (0, 0)
        );
        let first = visible_thumbnail_range_with_window(200, 0, MAX_THUMBNAILS_IN_FRAME);
        let middle = visible_thumbnail_range_with_window(200, 100, MAX_THUMBNAILS_IN_FRAME);
        let last = visible_thumbnail_range_with_window(200, 199, MAX_THUMBNAILS_IN_FRAME);
        for (start, end, current) in [
            (first.0, first.1, 0),
            (middle.0, middle.1, 100),
            (last.0, last.1, 199),
        ] {
            assert!(end > start);
            assert!(end - start <= MAX_THUMBNAILS_IN_FRAME);
            assert!((start..end).contains(&current));
        }
    }

    #[test]
    fn thumbnail_window_keeps_last_slide_visible_inside_small_canvas() {
        const CANVAS_HEIGHT: f32 = 423.0;
        let (start, end) = visible_thumbnail_range_for_viewport(19, 18, CANVAS_HEIGHT);
        assert_eq!((start, end), (16, 19));

        let selected_slot = 18 - start;
        let selected_item_bottom = THUMBNAIL_TOP_PADDING
            + THUMBNAIL_DRAW_TOP_INSET
            + (selected_slot + 1) as f32 * THUMBNAIL_ITEM_HEIGHT;
        assert!(selected_item_bottom <= CANVAS_HEIGHT);
    }
    fn evidence_plan(commands: Vec<RenderCommand>) -> RenderPlan {
        RenderPlan {
            slide_id: SlideId::new(7),
            slide_size: Size::new(1280.0, 720.0),
            viewport: Viewport::IDENTITY,
            commands,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn render_plan_summary_tracks_balanced_clip_commands_without_disclosure() {
        let rect = SlideRect::new(Point::ZERO, 120.0, 60.0);
        let plan = evidence_plan(vec![
            RenderCommand::Clear {
                color: [1.0, 1.0, 1.0, 1.0],
            },
            RenderCommand::FillRect {
                rect,
                color: [0.2, 0.2, 0.2, 1.0],
            },
            RenderCommand::DrawLine {
                start: Point::new(0.0, 0.0),
                end: Point::new(120.0, 60.0),
                color: [0.0, 0.0, 0.0, 1.0],
                width: 2.0,
            },
            RenderCommand::DrawText {
                rect,
                text: Text::plain("Кириллица в слайде"),
                color: [0.0, 0.0, 0.0, 1.0],
            },
            RenderCommand::DrawImage {
                rect,
                format: ImageFormat::Bmp,
                bytes: vec![1],
                alt_text: Some("Схема".to_owned()),
            },
            RenderCommand::PushClip { rect },
            RenderCommand::PopClip,
        ]);

        let summary = SlidesRenderPlanSummary::from_plan(&plan);
        assert_eq!(summary.total_commands, 7);
        assert_eq!(summary.considered_commands, 7);
        assert_eq!(summary.draw_commands, 5);
        assert_eq!(summary.text_commands, 1);
        assert_eq!(summary.image_commands, 1);
        assert_eq!(summary.graphic_commands, 1);
        assert_eq!(summary.line_commands, 1);
        assert_eq!(summary.clip_commands, 2);
        assert_eq!(summary.unbalanced_clip_commands, 0);
        assert!(!summary.needs_disclosure());
    }

    #[test]
    fn render_plan_summary_discloses_unbalanced_clip_commands() {
        let rect = SlideRect::new(Point::ZERO, 120.0, 60.0);
        let plan = evidence_plan(vec![
            RenderCommand::PushClip { rect },
            RenderCommand::DrawText {
                rect,
                text: Text::plain("Несбалансированная обрезка"),
                color: [0.0, 0.0, 0.0, 1.0],
            },
            RenderCommand::PopClip,
            RenderCommand::PopClip,
        ]);

        let summary = SlidesRenderPlanSummary::from_plan(&plan);
        assert_eq!(summary.clip_commands, 3);
        assert_eq!(summary.unbalanced_clip_commands, 1);
        assert!(summary.needs_disclosure());
    }

    #[test]
    fn semantic_items_expose_real_text_image_and_graphic_bounds() {
        let rect = SlideRect::new(Point::new(12.0, 18.0), 120.0, 60.0);
        let plan = evidence_plan(vec![
            RenderCommand::DrawText {
                rect,
                text: Text::plain("Проверка видимого текста"),
                color: [0.0, 0.0, 0.0, 1.0],
            },
            RenderCommand::DrawImage {
                rect,
                format: ImageFormat::Png,
                bytes: vec![1],
                alt_text: Some("Логотип".to_owned()),
            },
            RenderCommand::StrokeRect {
                rect,
                color: [0.0, 0.0, 0.0, 1.0],
                width: 1.0,
            },
        ]);
        let items = semantic_items_for_plan(
            &plan,
            dvec2(100.0, 200.0),
            Rect {
                pos: dvec2(0.0, 0.0),
                size: dvec2(500.0, 500.0),
            },
        );
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].kind, SlidesAccessibilityKind::Text);
        assert!(items[0].name.contains("Проверка видимого текста"));
        assert_eq!(items[1].kind, SlidesAccessibilityKind::Image);
        assert!(items[1].name.contains("Логотип"));
        assert_eq!(items[2].kind, SlidesAccessibilityKind::Graphic);
        assert!(items.iter().all(|item| item.bounds.size.x > 0.0));
        assert!(items.iter().all(|item| item.bounds.size.y > 0.0));
    }

    #[test]
    fn bmp_decoder_matches_the_render_plan_capability() {
        // 2x2 24-bit BMP. This follows Makepad's own supported fixture shape.
        let pixels: [u8; 16] = [30, 20, 10, 30, 20, 10, 0, 0, 30, 20, 10, 30, 20, 10, 0, 0];
        let mut bmp = Vec::new();
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(54u32 + pixels.len() as u32).to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&2i32.to_le_bytes());
        bmp.extend_from_slice(&2i32.to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&0u32.to_le_bytes());
        bmp.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
        bmp.extend_from_slice(&[0u8; 16]);
        bmp.extend_from_slice(&pixels);

        let image = decode_slide_image(ImageFormat::Bmp, &bmp)
            .expect("Makepad can decode the supported 24-bit BMP fixture");
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.data, vec![0xFF0A_141E; 4]);
    }

    #[test]
    fn large_jpeg_source_is_admitted_then_bounded_to_texture_budget() {
        // The first slide of the controlled acceptance PPTX contains this normal
        // baseline JPEG: 10,285,333 bytes and 5,568 x 3,712 pixels.
        let jpeg = vec![
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x08, 0x08, 0x0e, 0x80, 0x15, 0xc0, 0x03,
        ];
        assert_eq!(jpeg_dimensions(&jpeg), Some((5_568, 3_712)));
        assert!(image_byte_count_is_admissible(10_285_333));
        assert!(!image_byte_count_is_admissible(MAX_IMAGE_BYTES + 1));
        assert!(image_source_is_admissible(ImageFormat::Jpeg, &jpeg));

        let mut rgba = Vec::new();
        for value in 0..24u8 {
            rgba.extend_from_slice(&[value, 0, 0, 0xff]);
        }
        let image = ImageBuffer::new(&rgba, 6, 4).expect("small RGBA image is valid");
        let (texture, downsampled) = downsample_image_to_pixel_budget(image, 8);
        assert!(downsampled);
        assert!(texture.width * texture.height <= 8);
        assert_eq!(texture.data.len(), texture.width * texture.height);
    }
}
