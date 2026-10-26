use crate::PdfViewerFrame;
use crate::pdf_texture_identity::matches_pdf_texture;
use makepad_widgets::widget::WidgetActionData;
use makepad_widgets::*;
use pdf_core::{DocumentId, PageId};

const MAX_TEXTURES: usize = 8;

#[derive(Clone, Debug, Default)]
pub enum PdfPageCanvasAction {
    #[default]
    None,
    PageClicked(PageId),
    Wheel {
        delta_x: i64,
        delta_y: i64,
        control: bool,
    },
    KeyDown(KeyCode),
}

struct CachedTexture {
    document_id: DocumentId,
    page_index: PageId,
    scale_bits: u32,
    texture: Texture,
}

#[derive(Script, ScriptHook, Widget)]
pub struct PdfPageCanvas {
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
    draw_shadow: DrawColor,
    #[live]
    draw_page: DrawImage,
    #[live]
    draw_placeholder: DrawColor,
    #[live]
    draw_border: DrawColor,

    #[action_data]
    #[rust]
    action_data: WidgetActionData,
    #[rust]
    frame: Option<PdfViewerFrame>,
    #[rust]
    textures: Vec<CachedTexture>,
    #[rust]
    open_trace_draw_reported: bool,
}

impl PdfPageCanvas {
    pub fn set_frame(&mut self, cx: &mut Cx, frame: Option<PdfViewerFrame>) {
        let Some(next_frame) = frame.as_ref() else {
            self.frame = None;
            self.open_trace_draw_reported = false;
            self.textures.clear();
            self.draw_bg.redraw(cx);
            return;
        };

        let Some(document_id) = next_frame.document_id else {
            self.textures.clear();
            self.frame = frame;
            self.draw_bg.redraw(cx);
            return;
        };
        if self.frame.as_ref().and_then(|frame| frame.document_id) != next_frame.document_id {
            self.open_trace_draw_reported = false;
        }
        let scale_bits = next_frame.scale.to_bits();
        let mut next_textures = Vec::with_capacity(MAX_TEXTURES);
        for page in next_frame.pages.iter().take(MAX_TEXTURES) {
            if page.png.is_empty() {
                continue;
            }
            if let Some(cached) = self.textures.iter().find(|cached| {
                matches_pdf_texture(
                    cached.document_id,
                    cached.page_index,
                    cached.scale_bits,
                    document_id,
                    page.placement.page_index,
                    scale_bits,
                )
            }) {
                next_textures.push(CachedTexture {
                    document_id,
                    page_index: cached.page_index,
                    scale_bits: cached.scale_bits,
                    texture: cached.texture.clone(),
                });
                continue;
            }
            if let Ok(image) = ImageBuffer::from_png(&page.png) {
                next_textures.push(CachedTexture {
                    document_id,
                    page_index: page.placement.page_index,
                    scale_bits,
                    texture: image.into_new_texture(cx),
                });
            }
        }
        self.textures = next_textures;
        self.frame = frame;
        self.draw_bg.redraw(cx);
    }

    fn emit(&self, cx: &mut Cx, action: PdfPageCanvasAction) {
        cx.widget_action_with_data(&self.action_data, self.uid, action);
    }

    pub fn viewport_points(&self, cx: &Cx) -> (f32, f32) {
        let rect = self.draw_bg.area().rect(cx);
        (rect.size.x as f32, rect.size.y as f32)
    }

    fn page_at(&self, frame: &PdfViewerFrame, point: DVec2, canvas: Rect) -> Option<PageId> {
        let x = point.x - canvas.pos.x + frame.scroll_x_points as f64;
        let y = point.y - canvas.pos.y + frame.scroll_y_points as f64;
        frame.pages.iter().find_map(|page| {
            let placement = page.placement;
            (x >= placement.x_points as f64
                && x <= placement.x_points as f64 + placement.width_points as f64
                && y >= placement.y_points as f64
                && y <= placement.y_points as f64 + placement.height_points as f64)
                .then_some(placement.page_index)
        })
    }

    fn draw_rect(&mut self, cx: &mut Cx2d, rect: Rect, color: Vec4f) {
        self.draw_border.color = color;
        self.draw_border.draw_abs(cx, rect);
    }

    fn visible_on_canvas(rect: Rect, canvas: Rect) -> bool {
        rect.pos.x + rect.size.x >= canvas.pos.x
            && rect.pos.x <= canvas.pos.x + canvas.size.x
            && rect.pos.y + rect.size.y >= canvas.pos.y
            && rect.pos.y <= canvas.pos.y + canvas.size.y
    }
}

impl Widget for PdfPageCanvas {
    #[allow(clippy::collapsible_if)]
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, _scope: &mut Scope) {
        let area = self.draw_bg.area();
        match event.hits_with_capture_overload(cx, area, true) {
            Hit::FingerDown(fe) if fe.is_primary_hit() => {
                cx.set_key_focus(area);
                if let Some(frame) = self.frame.as_ref() {
                    if let Some(page_index) = self.page_at(frame, fe.abs, area.rect(cx)) {
                        self.emit(cx, PdfPageCanvasAction::PageClicked(page_index));
                    }
                }
            }
            Hit::FingerScroll(fe) => {
                self.emit(
                    cx,
                    PdfPageCanvasAction::Wheel {
                        delta_x: fe.scroll.x.round() as i64,
                        delta_y: fe.scroll.y.round() as i64,
                        control: fe.modifiers.control,
                    },
                );
            }
            Hit::KeyDown(ke) => {
                self.emit(cx, PdfPageCanvasAction::KeyDown(ke.key_code));
            }
            _ => {}
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let canvas = cx.walk_turtle(walk);
        self.draw_bg.color = Vec4f::from_u32(0x687384ff);
        self.draw_bg.draw_abs(cx, canvas);
        let Some(frame) = self.frame.clone() else {
            return DrawStep::done();
        };
        if !self.open_trace_draw_reported && std::env::var_os("RUDRA_DOCUMENT_OPEN_TRACE").is_some()
        {
            eprintln!(
                "RUDRA_OPEN stage=first_makepad_draw status=PASS format=pdf canvas={:.1}x{:.1} frame_pages={}",
                canvas.size.x,
                canvas.size.y,
                frame.pages.len()
            );
            self.open_trace_draw_reported = true;
        }

        for page in frame.pages.iter().take(MAX_TEXTURES) {
            let placement = page.placement;
            let page_rect = Rect {
                pos: dvec2(
                    canvas.pos.x + placement.x_points as f64 - frame.scroll_x_points as f64,
                    canvas.pos.y + placement.y_points as f64 - frame.scroll_y_points as f64,
                ),
                size: dvec2(
                    placement.width_points as f64,
                    placement.height_points as f64,
                ),
            };
            if !Self::visible_on_canvas(page_rect, canvas) {
                continue;
            }
            self.draw_shadow.color = Vec4f::from_u32(0x1b24304d);
            self.draw_shadow.draw_abs(
                cx,
                Rect {
                    pos: dvec2(page_rect.pos.x + 5.0, page_rect.pos.y + 6.0),
                    size: page_rect.size,
                },
            );

            if let Some(cached) = self.textures.iter().find(|cached| {
                matches_pdf_texture(
                    Some(cached.document_id),
                    cached.page_index,
                    cached.scale_bits,
                    frame.document_id,
                    placement.page_index,
                    frame.scale.to_bits(),
                )
            }) {
                self.draw_page.draw_vars.set_texture(0, &cached.texture);
                self.draw_page.image_dim_w = placement.width_points;
                self.draw_page.image_dim_h = placement.height_points;
                self.draw_page.draw_abs(cx, page_rect);
            } else {
                self.draw_placeholder.color = Vec4f::from_u32(0xffffffff);
                self.draw_placeholder.draw_abs(cx, page_rect);
            }
            self.draw_rect(
                cx,
                Rect {
                    pos: page_rect.pos,
                    size: dvec2(page_rect.size.x, 1.0),
                },
                if placement.page_index == frame.current_page {
                    Vec4f::from_u32(0x2f80edff)
                } else {
                    Vec4f::from_u32(0xd2d6dcff)
                },
            );
            self.draw_rect(
                cx,
                Rect {
                    pos: dvec2(page_rect.pos.x, page_rect.pos.y + page_rect.size.y - 1.0),
                    size: dvec2(page_rect.size.x, 1.0),
                },
                Vec4f::from_u32(0xd2d6dcff),
            );
            self.draw_rect(
                cx,
                Rect {
                    pos: page_rect.pos,
                    size: dvec2(1.0, page_rect.size.y),
                },
                Vec4f::from_u32(0xd2d6dcff),
            );
            self.draw_rect(
                cx,
                Rect {
                    pos: dvec2(page_rect.pos.x + page_rect.size.x - 1.0, page_rect.pos.y),
                    size: dvec2(1.0, page_rect.size.y),
                },
                Vec4f::from_u32(0xd2d6dcff),
            );
        }
        DrawStep::done()
    }
}
