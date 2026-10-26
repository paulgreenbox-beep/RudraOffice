//! Canvas-specific types built on top of the frozen `slides-api` and `slides-core` DTOs.
//!
//! `Point`, `Rect`, and `Transform` are re-exported from [`slides_api`];
//! canvas defines only the transient `Viewport` state and helper traits.

pub use slides_api::{Point, Rect, Transform};

/// Extension methods for [`Point`] that canvas needs but the frozen DTO lacks.
pub trait PointExt: sealed::Sealed {
    /// Returns `true` when both coordinates are finite.
    #[allow(clippy::wrong_self_convention)]
    fn is_finite(self) -> bool;
}

impl PointExt for Point {
    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// Extension methods for [`Rect`] that canvas needs but the frozen DTO lacks.
pub trait RectExt: sealed::Sealed {
    /// Returns the x-coordinate of the right edge.
    fn right(self) -> f32;
    /// Returns the y-coordinate of the bottom edge.
    fn bottom(self) -> f32;
    /// Returns the center point of the rectangle.
    fn center(self) -> Point;
    /// Returns a new rectangle expanded by `amount` on all sides.
    fn expanded(self, amount: f32) -> Rect;
    /// Transforms the rectangle by `transform` and returns finite axis-aligned bounds.
    ///
    /// Translation and scale preserve the canvas' existing `(coordinate +
    /// translation) * scale` semantics. The resulting rectangle is then rotated
    /// around its center. Invalid input returns an invalid rectangle instead of
    /// producing partially finite geometry.
    fn apply_rect(self, transform: Transform) -> Rect;
    /// Returns `true` when `point` is inside the rectangle.
    fn contains(self, point: Point) -> bool;
}

impl RectExt for Rect {
    fn right(self) -> f32 {
        self.origin.x + self.width
    }

    fn bottom(self) -> f32 {
        self.origin.y + self.height
    }

    fn center(self) -> Point {
        Point::new(
            self.origin.x + self.width * 0.5,
            self.origin.y + self.height * 0.5,
        )
    }

    fn expanded(self, amount: f32) -> Rect {
        Rect::new(
            Point::new(self.origin.x - amount, self.origin.y - amount),
            self.width + amount * 2.0,
            self.height + amount * 2.0,
        )
    }

    fn apply_rect(self, transform: Transform) -> Rect {
        let scaled = apply_rect_without_rotation(self, transform);
        if !scaled.is_valid() || !transform.rotation_degrees.is_finite() {
            return invalid_rect();
        }

        let center = scaled.center();
        let angle = transform.rotation_degrees.to_radians();
        let rotated = [
            rotate_point(Point::new(scaled.origin.x, scaled.origin.y), center, angle),
            rotate_point(Point::new(scaled.right(), scaled.origin.y), center, angle),
            rotate_point(Point::new(scaled.right(), scaled.bottom()), center, angle),
            rotate_point(Point::new(scaled.origin.x, scaled.bottom()), center, angle),
        ];
        bounds_of_points(rotated).unwrap_or_else(invalid_rect)
    }

    fn contains(self, point: Point) -> bool {
        point.x >= self.origin.x
            && point.x <= self.origin.x + self.width
            && point.y >= self.origin.y
            && point.y <= self.origin.y + self.height
    }
}

/// Applies translation and scale while retaining the canvas' existing
/// `(coordinate + translation) * scale` semantics, without rotation.
pub(crate) fn apply_rect_without_rotation(rect: Rect, transform: Transform) -> Rect {
    if !rect.is_valid()
        || !transform.translation.is_finite()
        || !transform.scale.is_finite()
        || transform.scale < 0.0
        || !transform.rotation_degrees.is_finite()
    {
        return invalid_rect();
    }
    Rect::new(
        Point::new(
            (rect.origin.x + transform.translation.x) * transform.scale,
            (rect.origin.y + transform.translation.y) * transform.scale,
        ),
        rect.width * transform.scale,
        rect.height * transform.scale,
    )
}

/// Rotates a finite point around a finite center by `angle` radians.
///
/// A non-finite angle or point is returned unchanged so callers can preserve a
/// safe miss/invalid result without panicking.
pub(crate) fn rotate_point(point: Point, center: Point, angle: f32) -> Point {
    if !point.is_finite() || !center.is_finite() || !angle.is_finite() {
        return point;
    }
    let (sin, cos) = angle.sin_cos();
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    Point::new(
        center.x + dx * cos - dy * sin,
        center.y + dx * sin + dy * cos,
    )
}

fn bounds_of_points(points: [Point; 4]) -> Option<Rect> {
    if !points.iter().all(|point| point.is_finite()) {
        return None;
    }
    let (min_x, max_x) = points
        .iter()
        .map(|point| point.x)
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), value| {
            (min.min(value), max.max(value))
        });
    let (min_y, max_y) = points
        .iter()
        .map(|point| point.y)
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), value| {
            (min.min(value), max.max(value))
        });
    let rect = Rect::new(Point::new(min_x, min_y), max_x - min_x, max_y - min_y);
    rect.is_valid().then_some(rect)
}

fn invalid_rect() -> Rect {
    Rect::new(Point::new(f32::NAN, f32::NAN), f32::NAN, f32::NAN)
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Point {}
    impl Sealed for super::Rect {}
}

/// Viewport is transient canvas state only. It never owns a document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub transform: Transform,
}

impl Default for Viewport {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Viewport {
    pub const IDENTITY: Self = Self {
        transform: Transform::IDENTITY,
    };
    pub const MIN_ZOOM: f32 = 0.25;
    pub const MAX_ZOOM: f32 = 4.0;

    pub const fn new(transform: Transform) -> Self {
        Self { transform }
    }

    pub fn with_zoom(self, scale: f32) -> Self {
        Self {
            transform: Transform::new(self.transform.translation, scale),
        }
    }

    pub fn pan_by(self, delta: Point) -> Self {
        Self {
            transform: Transform::new(
                Point::new(
                    self.transform.translation.x + delta.x,
                    self.transform.translation.y + delta.y,
                ),
                self.transform.scale,
            ),
        }
    }

    pub fn clamp_zoom(self, min_zoom: f32, max_zoom: f32) -> Self {
        let (min_zoom, max_zoom) = if min_zoom.is_finite()
            && max_zoom.is_finite()
            && min_zoom > 0.0
            && min_zoom <= max_zoom
        {
            (min_zoom, max_zoom)
        } else {
            (Self::MIN_ZOOM, Self::MAX_ZOOM)
        };
        let scale = if self.transform.scale.is_finite() {
            self.transform.scale.clamp(min_zoom, max_zoom)
        } else {
            min_zoom
        };
        Self {
            transform: Transform::new(self.transform.translation, scale),
        }
    }

    pub fn clamped(self) -> Self {
        self.clamp_zoom(Self::MIN_ZOOM, Self::MAX_ZOOM)
    }

    pub fn clamp_pan(self, content_bounds: Rect, viewport_bounds: Rect) -> Self {
        if !content_bounds.is_valid() || !viewport_bounds.is_valid() {
            return self;
        }

        let scale = self.transform.scale;
        if !scale.is_finite() || scale <= 0.0 {
            return self;
        }

        let content_width = content_bounds.width * scale;
        let content_height = content_bounds.height * scale;
        let viewport_width = viewport_bounds.width;
        let viewport_height = viewport_bounds.height;
        let translation_x = if content_width <= viewport_width {
            viewport_bounds.center().x
                - (content_bounds.origin.x + content_bounds.width * 0.5) * scale
        } else {
            let minimum = viewport_bounds.right() - content_bounds.right() * scale;
            let maximum = viewport_bounds.origin.x - content_bounds.origin.x * scale;
            self.transform.translation.x.clamp(minimum, maximum)
        };
        let translation_y = if content_height <= viewport_height {
            viewport_bounds.center().y
                - (content_bounds.origin.y + content_bounds.height * 0.5) * scale
        } else {
            let minimum = viewport_bounds.bottom() - content_bounds.bottom() * scale;
            let maximum = viewport_bounds.origin.y - content_bounds.origin.y * scale;
            self.transform.translation.y.clamp(minimum, maximum)
        };

        Self {
            transform: Transform::new(Point::new(translation_x, translation_y), scale),
        }
    }

    pub fn fit(content_bounds: Rect, viewport_bounds: Rect) -> Self {
        if !content_bounds.is_valid()
            || !viewport_bounds.is_valid()
            || content_bounds.width <= 0.0
            || content_bounds.height <= 0.0
            || viewport_bounds.width <= 0.0
            || viewport_bounds.height <= 0.0
        {
            return Self::IDENTITY;
        }

        let scale = (viewport_bounds.width / content_bounds.width)
            .min(viewport_bounds.height / content_bounds.height);
        let content_center = content_bounds.center();
        let viewport_center = viewport_bounds.center();
        let translation = Point::new(
            viewport_center.x / scale - content_center.x,
            viewport_center.y / scale - content_center.y,
        );

        if !scale.is_finite() || !translation.is_finite() {
            return Self::IDENTITY;
        }

        Self {
            transform: Transform::new(translation, scale),
        }
    }
}
