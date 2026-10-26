use slides_canvas::{Point, Rect, RenderCommand, RenderCommandEncoder};

#[derive(Default)]
pub struct TestEncoder {
    commands: Vec<RenderCommand>,
}

impl RenderCommandEncoder for TestEncoder {
    fn commands(&self) -> &[RenderCommand] {
        &self.commands
    }

    fn encode_clear(&mut self, color: [f32; 4]) {
        self.commands.push(RenderCommand::Clear { color });
    }

    fn encode_fill_rect(&mut self, rect: Rect, color: [f32; 4]) {
        self.commands.push(RenderCommand::FillRect { rect, color });
    }

    fn encode_stroke_rect(&mut self, rect: Rect, color: [f32; 4], width: f32) {
        self.commands
            .push(RenderCommand::StrokeRect { rect, color, width });
    }

    fn encode_line(&mut self, start: Point, end: Point, color: [f32; 4], width: f32) {
        self.commands.push(RenderCommand::DrawLine {
            start,
            end,
            color,
            width,
        });
    }
}

pub struct EmptyEncoder;

impl RenderCommandEncoder for EmptyEncoder {
    fn commands(&self) -> &[RenderCommand] {
        &[]
    }

    fn encode_clear(&mut self, _color: [f32; 4]) {}

    fn encode_fill_rect(&mut self, _rect: Rect, _color: [f32; 4]) {}

    fn encode_stroke_rect(&mut self, _rect: Rect, _color: [f32; 4], _width: f32) {}

    fn encode_line(&mut self, _start: Point, _end: Point, _color: [f32; 4], _width: f32) {}
}
