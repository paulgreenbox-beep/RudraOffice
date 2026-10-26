mod support;

use slides_api::{ImageFormat, Paragraph, Text, TextRun};
use slides_canvas::{
    BoundingBox, CanvasHost, CanvasInteraction, CanvasLayer, DragMode, Guides, HitTest,
    InteractionTransition, LockedHidden, Point, Rect, RectExt, RenderBackend, RenderBackendError,
    RenderCommand, RenderCommandEncoder, SelectionHandles, SnapTolerance, Transform, Viewport,
    guide, hit_test, interaction_commands, snap,
};

#[test]
fn clear_command_round_trip() {
    let backend = TestBackend;
    let mut encoder = backend.create_encoder();
    encoder.encode_clear([1.0, 0.0, 0.0, 1.0]);
    let commands = backend.present(encoder).expect("present succeeds");
    assert_eq!(
        commands,
        vec![RenderCommand::Clear {
            color: [1.0, 0.0, 0.0, 1.0],
        }]
    );
}

#[test]
fn rect_and_line_commands_are_deterministic() {
    let backend = TestBackend;
    let mut encoder = backend.create_encoder();
    encoder.encode_fill_rect(Rect::new(Point::ZERO, 10.0, 20.0), [0.2, 0.3, 0.4, 1.0]);
    encoder.encode_line(
        Point { x: 0.0, y: 0.0 },
        Point { x: 5.0, y: 5.0 },
        [1.0, 1.0, 1.0, 1.0],
        2.0,
    );
    let commands = backend.present(encoder).expect("present succeeds");
    assert_eq!(
        commands,
        vec![
            RenderCommand::FillRect {
                rect: Rect::new(Point::ZERO, 10.0, 20.0),
                color: [0.2, 0.3, 0.4, 1.0],
            },
            RenderCommand::DrawLine {
                start: Point::ZERO,
                end: Point { x: 5.0, y: 5.0 },
                color: [1.0, 1.0, 1.0, 1.0],
                width: 2.0,
            }
        ]
    );
}

#[test]
fn unsupported_command_propagates_error() {
    struct FailingBackend;
    impl RenderBackend for FailingBackend {
        type CommandEncoder = support::EmptyEncoder;

        fn create_encoder(&self) -> Self::CommandEncoder {
            support::EmptyEncoder
        }

        fn present(
            &self,
            _encoder: Self::CommandEncoder,
        ) -> Result<Vec<RenderCommand>, RenderBackendError> {
            Err(RenderBackendError::UnsupportedCommand)
        }
    }

    let host = TestHost::default();
    let layer = CanvasLayer::new(FailingBackend, host, SnapTolerance::ZERO);
    assert_eq!(layer.render(), Err(RenderBackendError::UnsupportedCommand));
}

#[test]
fn viewport_does_not_acquire_document_ownership() {
    struct PanningHost {
        viewport: Viewport,
    }

    impl Default for PanningHost {
        fn default() -> Self {
            Self {
                viewport: Viewport {
                    transform: Transform::new(Point::new(2.0, 3.0), 1.0),
                },
            }
        }
    }

    impl CanvasHost for PanningHost {
        fn viewport(&self) -> Viewport {
            self.viewport
        }

        fn apply_viewport(&mut self, viewport: Viewport) {
            self.viewport = viewport;
        }

        fn visible_bounds(&self) -> Rect {
            Rect::new(Point::ZERO, 100.0, 100.0)
        }

        fn locked_hidden_state(&self, _element_id: u64) -> LockedHidden {
            LockedHidden::VISIBLE_UNLOCKED
        }

        fn request_editor_command(
            &mut self,
            _command: CanvasInteraction,
        ) -> Result<(), RenderBackendError> {
            Ok(())
        }
    }

    let mut host = PanningHost::default();
    assert_eq!(
        host.viewport().transform.translation,
        Point { x: 2.0, y: 3.0 }
    );
    host.apply_viewport(Viewport {
        transform: Transform::new(Point::new(-1.0, 0.0), 2.0),
    });
    assert_eq!(host.viewport().transform.scale, 2.0);
}

#[test]
fn bounding_box_transform_scales_and_translates() {
    let bbox = BoundingBox::new(Rect::new(Point { x: 10.0, y: 20.0 }, 40.0, 50.0), 0.0);
    let transformed = bbox.transformed(Transform::new(Point { x: 5.0, y: 5.0 }, 2.0));
    assert_eq!(
        transformed.rect,
        Rect::new(Point { x: 30.0, y: 50.0 }, 80.0, 100.0)
    );
    assert_eq!(transformed.rotation_radians, 0.0);
}

#[test]
fn transformed_rect_bounds_cover_90_and_45_degree_rotations() {
    let rect = Rect::new(Point::ZERO, 40.0, 20.0);
    assert_rect_close(
        rect.apply_rect(Transform::with_rotation(Point::ZERO, 1.0, 90.0)),
        Rect::new(Point::new(10.0, -10.0), 20.0, 40.0),
    );
    assert_rect_close(
        rect.apply_rect(Transform::with_rotation(Point::ZERO, 1.0, 45.0)),
        Rect::new(Point::new(-1.213203, -11.213203), 42.426407, 42.426407),
    );
}

#[test]
fn bounding_box_rotation_produces_rotated_corners_without_double_bounds() {
    let bbox = BoundingBox::new(Rect::new(Point::ZERO, 40.0, 20.0), 0.0);
    let transformed = bbox.transformed(Transform::with_rotation(Point::ZERO, 1.0, 90.0));
    assert_rect_close(transformed.rect, Rect::new(Point::ZERO, 40.0, 20.0));
    assert_points_close(
        transformed.corners(),
        [
            Point::new(30.0, -10.0),
            Point::new(30.0, 30.0),
            Point::new(10.0, 30.0),
            Point::new(10.0, -10.0),
        ],
    );

    let diagonal = BoundingBox::new(Rect::new(Point::ZERO, 40.0, 20.0), 45.0_f32.to_radians());
    assert_points_close(
        diagonal.corners(),
        [
            Point::new(12.928932, -11.213203),
            Point::new(41.213203, 17.071068),
            Point::new(27.071068, 31.213203),
            Point::new(-1.213203, 2.928932),
        ],
    );
}

#[test]
fn bounding_box_transform_composes_rotation_without_double_application() {
    let bbox = BoundingBox::new(
        Rect::new(Point::new(10.0, 20.0), 40.0, 20.0),
        30.0_f32.to_radians(),
    );
    let transformed = bbox.transformed(Transform::with_rotation(Point::new(5.0, 5.0), 2.0, 15.0));
    assert_rect_close(
        transformed.rect,
        Rect::new(Point::new(30.0, 50.0), 80.0, 40.0),
    );
    assert_close(transformed.rotation_radians, 45.0_f32.to_radians());
    assert_points_close(
        transformed.corners(),
        [
            Point::new(55.857864, 27.573593),
            Point::new(112.426_41, 84.142136),
            Point::new(84.142136, 112.426_41),
            Point::new(27.573593, 55.857864),
        ],
    );
}

#[test]
fn rotated_selection_handles_and_body_hit_testing_follow_geometry() {
    let handles = SelectionHandles::from_bounding_box(BoundingBox::new(
        Rect::new(Point::ZERO, 20.0, 10.0),
        45.0_f32.to_radians(),
    ));
    assert_points_close(
        [
            handles.resize_handle(),
            handles.rotate_handle(),
            Point::ZERO,
            Point::ZERO,
        ],
        [
            Point::new(13.535534, 15.606602),
            Point::new(19.192387, -4.192388),
            Point::ZERO,
            Point::ZERO,
        ],
    );
    assert_eq!(
        hit_test(&handles, handles.resize_handle(), SnapTolerance::ZERO),
        HitTest::HandleResize
    );
    assert_eq!(
        hit_test(&handles, handles.rotate_handle(), SnapTolerance::ZERO),
        HitTest::HandleRotate
    );
    assert_eq!(
        hit_test(&handles, Point::new(10.0, 5.0), SnapTolerance::ZERO),
        HitTest::Body
    );
    assert_eq!(
        hit_test(&handles, Point::new(10.0, 20.0), SnapTolerance::ZERO),
        HitTest::Miss
    );
}

#[test]
fn invalid_rotation_is_safe_for_transforms_and_hit_testing() {
    let bbox = BoundingBox::new(Rect::new(Point::ZERO, 10.0, 10.0), 0.0);
    let transformed = bbox.transformed(Transform::with_rotation(Point::ZERO, 1.0, f32::NAN));
    assert!(!transformed.rect.is_valid());
    assert_eq!(
        hit_test(
            &SelectionHandles::from_bounding_box(transformed),
            Point::ZERO,
            SnapTolerance::ZERO,
        ),
        HitTest::Miss
    );
}

#[test]
fn hit_test_body_detection_with_tolerance() {
    let handles = SelectionHandles::from_bounding_box(BoundingBox::new(
        Rect::new(Point { x: 0.0, y: 0.0 }, 10.0, 10.0),
        0.0,
    ));
    let tolerance = SnapTolerance {
        x: 1.0,
        y: 1.0,
        rotation: 1.0,
    };
    assert_eq!(
        hit_test(&handles, Point { x: 11.0, y: 5.0 }, tolerance),
        HitTest::Body
    );
    assert_eq!(
        hit_test(&handles, Point { x: 12.0, y: 5.0 }, tolerance),
        HitTest::Miss
    );
}

#[test]
fn hidden_element_suppresses_render_commands() {
    let interaction = CanvasInteraction {
        element_id: 1,
        point: Point { x: 0.0, y: 0.0 },
        locked_hidden: LockedHidden {
            locked: false,
            hidden: true,
        },
    };
    assert!(interaction_commands(&interaction).is_empty());
}

#[test]
fn snap_respects_tolerance() {
    assert_eq!(
        snap(
            9.9,
            10.0,
            SnapTolerance {
                x: 0.2,
                y: 0.0,
                rotation: 0.0
            }
        ),
        10.0
    );
    assert_eq!(
        snap(
            9.4,
            10.0,
            SnapTolerance {
                x: 0.2,
                y: 0.0,
                rotation: 0.0
            }
        ),
        9.4
    );
}

#[test]
fn viewport_pan_clamp_keeps_content_inside_viewport() {
    let viewport = Viewport::new(Transform::new(Point::new(0.0, 0.0), 2.0)).clamp_pan(
        Rect::new(Point::ZERO, 100.0, 100.0),
        Rect::new(Point::ZERO, 200.0, 200.0),
    );
    assert_eq!(viewport.transform.translation, Point::ZERO);

    let viewport = Viewport::new(Transform::new(Point::new(500.0, 500.0), 2.0)).clamp_pan(
        Rect::new(Point::ZERO, 100.0, 100.0),
        Rect::new(Point::ZERO, 200.0, 200.0),
    );
    assert_eq!(viewport.transform.translation, Point::ZERO);
}

#[test]
fn zoom_pan_clamp_does_not_exceed_limits() {
    struct ClampHost {
        viewport: Viewport,
    }

    impl Default for ClampHost {
        fn default() -> Self {
            Self {
                viewport: Viewport::IDENTITY,
            }
        }
    }

    impl CanvasHost for ClampHost {
        fn viewport(&self) -> Viewport {
            self.viewport
        }

        fn apply_viewport(&mut self, viewport: Viewport) {
            let clamped_scale = viewport.transform.scale.clamp(0.25, 4.0);
            let clamped = Viewport {
                transform: Transform::new(viewport.transform.translation, clamped_scale),
            };
            self.viewport = clamped;
        }

        fn visible_bounds(&self) -> Rect {
            Rect::new(Point::ZERO, 100.0, 100.0)
        }

        fn locked_hidden_state(&self, _element_id: u64) -> LockedHidden {
            LockedHidden::VISIBLE_UNLOCKED
        }

        fn request_editor_command(
            &mut self,
            _command: CanvasInteraction,
        ) -> Result<(), RenderBackendError> {
            Ok(())
        }
    }

    let mut host = ClampHost::default();
    host.apply_viewport(Viewport {
        transform: Transform::new(Point { x: 0.0, y: 0.0 }, 10.0),
    });
    assert_eq!(host.viewport().transform.scale, 4.0);
}

#[test]
fn hit_test_prioritizes_selection_handles() {
    let handles = SelectionHandles::from_bounding_box(BoundingBox::new(
        Rect::new(Point::ZERO, 10.0, 10.0),
        0.0,
    ));
    assert_eq!(
        hit_test(&handles, Point::new(10.0, 10.0), SnapTolerance::ZERO),
        HitTest::HandleResize
    );
    assert_eq!(
        hit_test(&handles, Point::new(5.0, -8.0), SnapTolerance::ZERO),
        HitTest::HandleRotate
    );
}

#[test]
fn move_resize_rotate_interaction_transitions() {
    let transition = InteractionTransition::Dragging {
        mode: DragMode::Rotate,
        start: Point { x: 1.0, y: 2.0 },
    };
    assert_ne!(transition, InteractionTransition::IDLE);
}

#[test]
fn locked_elements_are_not_dispatched() {
    #[derive(Default)]
    struct Host {
        requests: usize,
    }

    impl CanvasHost for Host {
        fn viewport(&self) -> Viewport {
            Viewport::IDENTITY
        }

        fn apply_viewport(&mut self, _viewport: Viewport) {}

        fn visible_bounds(&self) -> Rect {
            Rect::new(Point::ZERO, 100.0, 100.0)
        }

        fn locked_hidden_state(&self, _element_id: u64) -> LockedHidden {
            LockedHidden {
                locked: true,
                hidden: false,
            }
        }

        fn request_editor_command(
            &mut self,
            _command: CanvasInteraction,
        ) -> Result<(), RenderBackendError> {
            self.requests += 1;
            Ok(())
        }
    }

    let mut layer = CanvasLayer::new(TestBackend, Host::default(), SnapTolerance::ZERO);
    layer
        .dispatch_interaction(CanvasInteraction {
            element_id: 7,
            point: Point::ZERO,
            locked_hidden: LockedHidden::VISIBLE_UNLOCKED,
        })
        .unwrap();
    assert_eq!(layer.host.requests, 0);
}

#[test]
fn guides_are_sorted_and_deduped() {
    let guides = Guides {
        horizontal: vec![10.0, 20.0, 10.0],
        vertical: vec![5.0, 5.0, 15.0],
    };
    assert_eq!(guide::sorted_unique(guides.horizontal), vec![10.0, 20.0]);
}

#[test]
fn draw_text_validates_rect_color_and_runs() {
    let text = Text {
        paragraphs: vec![Paragraph {
            runs: vec![TextRun::new("hello")],
            ..Paragraph::default()
        }],
    };
    let command = RenderCommand::DrawText {
        rect: Rect::new(Point::ZERO, 10.0, 10.0),
        text: text.clone(),
        color: [1.0, 1.0, 1.0, 1.0],
    };
    assert!(command.is_valid());

    let invalid_rect = RenderCommand::DrawText {
        rect: Rect::new(Point::new(f32::NAN, 0.0), 10.0, 10.0),
        text,
        color: [1.0, 1.0, 1.0, 1.0],
    };
    assert!(!invalid_rect.is_valid());

    let invalid_color = RenderCommand::DrawText {
        rect: Rect::new(Point::ZERO, 10.0, 10.0),
        text: Text::default(),
        color: [1.0, 1.0, 1.0, f32::NAN],
    };
    assert!(!invalid_color.is_valid());

    let empty_runs = RenderCommand::DrawText {
        rect: Rect::new(Point::ZERO, 10.0, 10.0),
        text: Text {
            paragraphs: vec![Paragraph::default()],
        },
        color: [1.0, 1.0, 1.0, 1.0],
    };
    assert!(!empty_runs.is_valid());

    let empty_run_text = RenderCommand::DrawText {
        rect: Rect::new(Point::ZERO, 10.0, 10.0),
        text: Text {
            paragraphs: vec![Paragraph {
                runs: vec![TextRun::new("")],
                ..Paragraph::default()
            }],
        },
        color: [1.0, 1.0, 1.0, 1.0],
    };
    assert!(!empty_run_text.is_valid());
}

#[test]
fn draw_image_validates_geometry_bytes_and_format() {
    let valid = RenderCommand::DrawImage {
        rect: Rect::new(Point::ZERO, 10.0, 10.0),
        format: ImageFormat::Png,
        bytes: vec![1, 2, 3],
        alt_text: None,
    };
    assert!(valid.is_valid());

    let invalid_rect = RenderCommand::DrawImage {
        rect: Rect::new(Point::new(f32::NAN, 0.0), 10.0, 10.0),
        format: ImageFormat::Jpeg,
        bytes: vec![1],
        alt_text: None,
    };
    assert!(!invalid_rect.is_valid());

    let empty_bytes = RenderCommand::DrawImage {
        rect: Rect::new(Point::ZERO, 10.0, 10.0),
        format: ImageFormat::Bmp,
        bytes: vec![],
        alt_text: None,
    };
    assert!(!empty_bytes.is_valid());

    let unsupported = RenderCommand::DrawImage {
        rect: Rect::new(Point::ZERO, 10.0, 10.0),
        format: ImageFormat::Svg,
        bytes: vec![1],
        alt_text: None,
    };
    assert!(!unsupported.is_valid());

    let unknown = RenderCommand::DrawImage {
        rect: Rect::new(Point::ZERO, 10.0, 10.0),
        format: ImageFormat::Unknown,
        bytes: vec![1],
        alt_text: None,
    };
    assert!(!unknown.is_valid());
}

#[test]
fn clip_commands_validate_geometry() {
    let push_valid = RenderCommand::PushClip {
        rect: Rect::new(Point::ZERO, 10.0, 10.0),
    };
    assert!(push_valid.is_valid());

    let push_invalid = RenderCommand::PushClip {
        rect: Rect::new(Point::new(f32::NAN, 0.0), 10.0, 10.0),
    };
    assert!(!push_invalid.is_valid());

    assert!(RenderCommand::PopClip.is_valid());
}

#[test]
fn balanced_clip_stack_is_consumer_responsibility() {
    let commands = [
        RenderCommand::PushClip {
            rect: Rect::new(Point::ZERO, 10.0, 10.0),
        },
        RenderCommand::FillRect {
            rect: Rect::new(Point::ZERO, 5.0, 5.0),
            color: [1.0, 1.0, 1.0, 1.0],
        },
        RenderCommand::PopClip,
    ];
    assert!(commands.iter().all(|command| command.is_valid()));
    assert_eq!(commands.len(), 3);
}

#[test]
fn viewport_fit_wide_content_centers_vertically() {
    let content = Rect::new(Point::ZERO, 200.0, 100.0);
    let viewport_bounds = Rect::new(Point::ZERO, 100.0, 100.0);
    let viewport = Viewport::fit(content, viewport_bounds);
    assert_eq!(viewport.transform.scale, 0.5);
    assert_eq!(viewport.transform.translation, Point::new(0.0, 50.0));

    let viewport_center = viewport_bounds.center();
    let content_center = content.center();
    let transformed_center = Point::new(
        (content_center.x + viewport.transform.translation.x) * viewport.transform.scale,
        (content_center.y + viewport.transform.translation.y) * viewport.transform.scale,
    );
    assert_eq!(transformed_center, viewport_center);
}

#[test]
fn viewport_fit_tall_content_centers_horizontally() {
    let content = Rect::new(Point::ZERO, 100.0, 200.0);
    let viewport_bounds = Rect::new(Point::ZERO, 100.0, 100.0);
    let viewport = Viewport::fit(content, viewport_bounds);
    assert_eq!(viewport.transform.scale, 0.5);
    assert_eq!(viewport.transform.translation, Point::new(50.0, 0.0));

    let viewport_center = viewport_bounds.center();
    let content_center = content.center();
    let transformed_center = Point::new(
        (content_center.x + viewport.transform.translation.x) * viewport.transform.scale,
        (content_center.y + viewport.transform.translation.y) * viewport.transform.scale,
    );
    assert_eq!(transformed_center, viewport_center);
}

#[test]
fn viewport_fit_returns_identity_for_invalid_inputs() {
    assert_eq!(
        Viewport::fit(
            Rect::new(Point::ZERO, 0.0, 100.0),
            Rect::new(Point::ZERO, 100.0, 100.0)
        ),
        Viewport::IDENTITY
    );
    assert_eq!(
        Viewport::fit(
            Rect::new(Point::ZERO, 100.0, 100.0),
            Rect::new(Point::ZERO, 100.0, 0.0)
        ),
        Viewport::IDENTITY
    );
    assert_eq!(
        Viewport::fit(
            Rect::new(Point::new(f32::NAN, 0.0), 100.0, 100.0),
            Rect::new(Point::ZERO, 100.0, 100.0)
        ),
        Viewport::IDENTITY
    );
    assert_eq!(
        Viewport::fit(
            Rect::new(Point::ZERO, 100.0, 100.0),
            Rect::new(Point::new(f32::NAN, 0.0), 100.0, 100.0)
        ),
        Viewport::IDENTITY
    );
}

fn assert_close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
}

fn assert_rect_close(actual: Rect, expected: Rect) {
    assert_points_close(
        [
            actual.origin,
            Point::new(
                actual.origin.x + actual.width,
                actual.origin.y + actual.height,
            ),
            Point::ZERO,
            Point::ZERO,
        ],
        [
            expected.origin,
            Point::new(
                expected.origin.x + expected.width,
                expected.origin.y + expected.height,
            ),
            Point::ZERO,
            Point::ZERO,
        ],
    );
    assert_close(actual.width, expected.width);
    assert_close(actual.height, expected.height);
}

fn assert_points_close(actual: [Point; 4], expected: [Point; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert_close(actual.x, expected.x);
        assert_close(actual.y, expected.y);
    }
}

struct TestBackend;

impl RenderBackend for TestBackend {
    type CommandEncoder = support::TestEncoder;

    fn create_encoder(&self) -> Self::CommandEncoder {
        support::TestEncoder::default()
    }

    fn present(
        &self,
        encoder: Self::CommandEncoder,
    ) -> Result<Vec<RenderCommand>, RenderBackendError> {
        Ok(encoder.commands().to_vec())
    }
}

#[derive(Default)]
struct TestHost {
    viewport: Viewport,
}

impl CanvasHost for TestHost {
    fn viewport(&self) -> Viewport {
        self.viewport
    }

    fn apply_viewport(&mut self, viewport: Viewport) {
        self.viewport = viewport;
    }

    fn visible_bounds(&self) -> Rect {
        Rect::new(Point::ZERO, 100.0, 100.0)
    }

    fn locked_hidden_state(&self, _element_id: u64) -> LockedHidden {
        LockedHidden::VISIBLE_UNLOCKED
    }

    fn request_editor_command(
        &mut self,
        _command: CanvasInteraction,
    ) -> Result<(), RenderBackendError> {
        Ok(())
    }
}
