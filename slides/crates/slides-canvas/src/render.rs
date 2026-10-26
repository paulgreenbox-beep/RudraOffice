use slides_api::{ImageFormat, Point, Rect, Text};

/// Error type returned by render backend boundary operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderBackendError {
    UnsupportedCommand,
    HostError,
}

/// GPU-neutral deterministic render command list.
#[derive(Debug, Clone, PartialEq)]
pub enum RenderCommand {
    Clear {
        color: [f32; 4],
    },
    FillRect {
        rect: Rect,
        color: [f32; 4],
    },
    StrokeRect {
        rect: Rect,
        color: [f32; 4],
        width: f32,
    },
    FillEllipse {
        rect: Rect,
        color: [f32; 4],
    },
    StrokeEllipse {
        rect: Rect,
        color: [f32; 4],
        width: f32,
    },
    FillChevron {
        rect: Rect,
        color: [f32; 4],
    },
    StrokeChevron {
        rect: Rect,
        color: [f32; 4],
        width: f32,
    },
    DrawLine {
        start: Point,
        end: Point,
        color: [f32; 4],
        width: f32,
    },
    DrawText {
        rect: Rect,
        text: Text,
        color: [f32; 4],
    },
    DrawImage {
        rect: Rect,
        format: ImageFormat,
        bytes: Vec<u8>,
        alt_text: Option<String>,
    },
    PushClip {
        rect: Rect,
    },
    PopClip,
}

impl RenderCommand {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Clear { color } => color.iter().all(|value| value.is_finite()),
            Self::FillRect { rect, color } => {
                rect.is_valid() && color.iter().all(|value| value.is_finite())
            }
            Self::StrokeRect { rect, color, width } => {
                rect.is_valid()
                    && color.iter().all(|value| value.is_finite())
                    && width.is_finite()
                    && *width >= 0.0
            }
            Self::FillEllipse { rect, color } => {
                rect.is_valid() && color.iter().all(|value| value.is_finite())
            }
            Self::StrokeEllipse { rect, color, width } => {
                rect.is_valid()
                    && color.iter().all(|value| value.is_finite())
                    && width.is_finite()
                    && *width >= 0.0
            }
            Self::FillChevron { rect, color } => {
                rect.is_valid() && color.iter().all(|value| value.is_finite())
            }
            Self::StrokeChevron { rect, color, width } => {
                rect.is_valid()
                    && color.iter().all(|value| value.is_finite())
                    && width.is_finite()
                    && *width >= 0.0
            }
            Self::DrawLine {
                start,
                end,
                color,
                width,
            } => {
                start.x.is_finite()
                    && start.y.is_finite()
                    && end.x.is_finite()
                    && end.y.is_finite()
                    && color.iter().all(|value| value.is_finite())
                    && width.is_finite()
                    && *width >= 0.0
            }
            Self::DrawText { rect, text, color } => {
                rect.is_valid()
                    && color.iter().all(|value| value.is_finite())
                    && text
                        .paragraphs
                        .iter()
                        .any(|paragraph| paragraph.runs.iter().any(|run| !run.text.is_empty()))
            }
            Self::DrawImage {
                rect,
                format,
                bytes,
                ..
            } => {
                rect.is_valid()
                    && matches!(
                        format,
                        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Bmp
                    )
                    && !bytes.is_empty()
            }
            Self::PushClip { rect } => rect.is_valid(),
            Self::PopClip => true,
        }
    }
}

pub trait RenderCommandEncoder {
    fn commands(&self) -> &[RenderCommand];

    fn encode_clear(&mut self, color: [f32; 4]);
    fn encode_fill_rect(&mut self, rect: Rect, color: [f32; 4]);
    fn encode_stroke_rect(&mut self, rect: Rect, color: [f32; 4], width: f32);
    fn encode_line(&mut self, start: Point, end: Point, color: [f32; 4], width: f32);
}

/// Minimal Makepad-ready headless boundary.
pub trait RenderBackend {
    type CommandEncoder: RenderCommandEncoder;

    fn create_encoder(&self) -> Self::CommandEncoder;
    fn present(
        &self,
        encoder: Self::CommandEncoder,
    ) -> Result<Vec<RenderCommand>, RenderBackendError>;
}
