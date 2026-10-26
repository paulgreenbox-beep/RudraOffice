use super::render::RenderCommand;
use crate::types::{PointExt, RectExt, apply_rect_without_rotation, rotate_point};
use slides_api::{Point, Rect, Transform};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub rect: Rect,
    pub rotation_radians: f32,
}

impl BoundingBox {
    pub const fn new(rect: Rect, rotation_radians: f32) -> Self {
        Self {
            rect,
            rotation_radians,
        }
    }

    /// Applies translation and scale to the local rectangle while composing
    /// rotation metadata. `rect` remains the pre-rotation transformed rect;
    /// [`Self::corners`] is the single place where that rotation is applied.
    pub fn transformed(&self, transform: Transform) -> Self {
        let rect = apply_rect_without_rotation(self.rect, transform);
        let rotation_radians = if rect.is_valid() && transform.rotation_degrees.is_finite() {
            self.rotation_radians + transform.rotation_degrees.to_radians()
        } else {
            f32::NAN
        };
        Self::new(rect, rotation_radians)
    }

    pub fn corners(self) -> [Point; 4] {
        let corners = [
            self.rect.origin,
            Point::new(self.rect.right(), self.rect.origin.y),
            Point::new(self.rect.right(), self.rect.bottom()),
            Point::new(self.rect.origin.x, self.rect.bottom()),
        ];
        if !self.rect.is_valid() || !self.rotation_radians.is_finite() {
            return corners;
        }
        let center = self.rect.center();
        corners.map(|corner| rotate_point(corner, center, self.rotation_radians))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockedHidden {
    pub locked: bool,
    pub hidden: bool,
}

impl LockedHidden {
    pub const VISIBLE_UNLOCKED: Self = Self {
        locked: false,
        hidden: false,
    };

    pub const fn is_interactive(self) -> bool {
        !self.locked && !self.hidden
    }

    pub const fn is_renderable(self) -> bool {
        !self.hidden
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragMode {
    None,
    Move,
    ResizeRight,
    ResizeBottom,
    Rotate,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InteractionTransition {
    Idle,
    Dragging { mode: DragMode, start: Point },
}

impl InteractionTransition {
    pub const IDLE: Self = Self::Idle;

    pub const fn begin(mode: DragMode, start: Point) -> Self {
        Self::Dragging { mode, start }
    }

    pub const fn finish(self) -> Self {
        Self::Idle
    }

    pub const fn is_dragging(self) -> bool {
        matches!(self, Self::Dragging { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionHandles {
    pub bounding_box: BoundingBox,
    pub handle_size: f32,
}

impl SelectionHandles {
    pub const DEFAULT_HANDLE_SIZE: f32 = 8.0;

    pub fn from_bounding_box(bounding_box: BoundingBox) -> Self {
        Self {
            bounding_box,
            handle_size: Self::DEFAULT_HANDLE_SIZE,
        }
    }

    pub fn with_handle_size(mut self, handle_size: f32) -> Self {
        if handle_size.is_finite() && handle_size >= 0.0 {
            self.handle_size = handle_size;
        }
        self
    }

    pub fn resize_handle(&self) -> Point {
        self.bounding_box
            .corners()
            .get(2)
            .copied()
            .unwrap_or(Point::new(f32::NAN, f32::NAN))
    }

    pub fn rotate_handle(&self) -> Point {
        let rect = self.bounding_box.rect;
        let center = rect.center();
        let local = Point::new(center.x, rect.origin.y - self.handle_size);
        if !rect.is_valid() || !self.bounding_box.rotation_radians.is_finite() {
            Point::new(f32::NAN, f32::NAN)
        } else {
            rotate_point(local, center, self.bounding_box.rotation_radians)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapTolerance {
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
}

impl SnapTolerance {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        rotation: 0.0,
    };

    pub const fn new(x: f32, y: f32, rotation: f32) -> Self {
        Self { x, y, rotation }
    }

    pub fn distance(self, axis: SnapAxis) -> f32 {
        let value = match axis {
            SnapAxis::Horizontal => self.x,
            SnapAxis::Vertical => self.y,
            SnapAxis::Rotation => self.rotation,
        };
        if value.is_finite() { value.abs() } else { 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapAxis {
    Horizontal,
    Vertical,
    Rotation,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Guides {
    pub horizontal: Vec<f32>,
    pub vertical: Vec<f32>,
}

impl Guides {
    pub fn sorted_unique(mut self) -> Self {
        self.horizontal = guide::sorted_unique(self.horizontal);
        self.vertical = guide::sorted_unique(self.vertical);
        self
    }

    pub fn add_horizontal(&mut self, value: f32) {
        self.horizontal.push(value);
        self.horizontal = guide::sorted_unique(std::mem::take(&mut self.horizontal));
    }

    pub fn add_vertical(&mut self, value: f32) {
        self.vertical.push(value);
        self.vertical = guide::sorted_unique(std::mem::take(&mut self.vertical));
    }
}

pub fn snap(value: f32, snap_value: f32, tolerance: SnapTolerance) -> f32 {
    snap_axis(value, snap_value, tolerance, SnapAxis::Horizontal)
}

pub fn snap_axis(value: f32, snap_value: f32, tolerance: SnapTolerance, axis: SnapAxis) -> f32 {
    if value.is_finite()
        && snap_value.is_finite()
        && (value - snap_value).abs() <= tolerance.distance(axis)
    {
        snap_value
    } else {
        value
    }
}

pub mod guide {
    pub fn sorted_unique(mut values: Vec<f32>) -> Vec<f32> {
        values.retain(|value| value.is_finite());
        values.sort_by(|left, right| left.total_cmp(right));
        values.dedup_by(|left, right| left == right);
        values
    }

    pub fn nearest(value: f32, guides: &[f32], tolerance: f32) -> Option<f32> {
        if !value.is_finite() || !tolerance.is_finite() {
            return None;
        }
        guides
            .iter()
            .copied()
            .filter(|guide| guide.is_finite())
            .filter(|guide| (value - guide).abs() <= tolerance.abs())
            .min_by(|left, right| {
                (value - *left)
                    .abs()
                    .total_cmp(&(value - *right).abs())
                    .then_with(|| left.total_cmp(right))
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTest {
    Miss,
    Body,
    HandleResize,
    HandleRotate,
}

pub fn hit_test(handles: &SelectionHandles, point: Point, tolerance: SnapTolerance) -> HitTest {
    if !point.is_finite()
        || !handles.bounding_box.rect.is_valid()
        || !handles.bounding_box.rotation_radians.is_finite()
    {
        return HitTest::Miss;
    }

    let handle_tolerance = tolerance.x.abs().max(tolerance.y.abs());
    let resize = handles.resize_handle();
    if Rect::new(resize, 0.0, 0.0)
        .expanded(handle_tolerance.max(handles.handle_size * 0.5))
        .contains(point)
    {
        return HitTest::HandleResize;
    }

    let rotate = handles.rotate_handle();
    if Rect::new(rotate, 0.0, 0.0)
        .expanded(handle_tolerance.max(handles.handle_size * 0.5))
        .contains(point)
    {
        return HitTest::HandleRotate;
    }

    let center = handles.bounding_box.rect.center();
    let body_point = rotate_point(point, center, -handles.bounding_box.rotation_radians);
    if handles
        .bounding_box
        .rect
        .expanded(handle_tolerance)
        .contains(body_point)
    {
        HitTest::Body
    } else {
        HitTest::Miss
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasInteraction {
    pub element_id: u64,
    pub point: Point,
    pub locked_hidden: LockedHidden,
}

impl CanvasInteraction {
    pub const fn is_dispatchable(self) -> bool {
        self.locked_hidden.is_interactive()
    }
}

pub fn interaction_commands(interaction: &CanvasInteraction) -> Vec<RenderCommand> {
    if !interaction.locked_hidden.is_renderable() {
        return Vec::new();
    }
    vec![RenderCommand::FillRect {
        rect: Rect::new(interaction.point, 1.0, 1.0),
        color: [0.1, 0.9, 0.3, 1.0],
    }]
}
