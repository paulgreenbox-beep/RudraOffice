use slides_api::{
    Capability, EngineError, ObjectId, ObjectKind, Paragraph, Presentation, PresentationEngine,
    PresentationId, Rect, Shape, ShapeKind, Slide, SlideId, SlideObject, Text, Transform,
};
use slides_pptx_adapter::{
    BuiltinCodec, Limits, PptxAdapter, PresentationCodec, RSLD_MAGIC, office_oxide_adapter,
    office_oxide_adapter_with_limits,
};

fn sample() -> Presentation {
    let mut slide = Slide::new(SlideId::new(1));
    slide
        .add_object(SlideObject::new(
            ObjectId::new(2),
            ObjectKind::Text(Text {
                paragraphs: vec![Paragraph::plain("integration")],
            }),
            Rect::new(slides_api::Point::ZERO, 200.0, 40.0),
            Transform::IDENTITY,
        ))
        .unwrap();
    let mut presentation = Presentation::new(PresentationId::new(10));
    presentation.add_slide(slide).unwrap();
    presentation
}

#[test]
fn public_engine_contract_round_trips_bytes() {
    let engine = PptxAdapter::default();
    let bytes = PresentationEngine::save(&engine, &sample()).unwrap();
    let decoded = PresentationEngine::load(&engine, &bytes).unwrap();
    assert_eq!(decoded, sample());
    assert_eq!(engine.capabilities(), engine.supported_capabilities());
    assert!(engine.supported_capabilities().contains(&Capability::Text));
}

#[test]
fn malformed_byte_corpus_is_deterministically_rejected_without_panic() {
    let codec = BuiltinCodec;
    let limits = Limits::default();
    for seed in 0_u16..=255 {
        let length = usize::from(seed % 73);
        let input: Vec<u8> = (0..length)
            .map(|index| (u16::try_from(index).unwrap() ^ seed) as u8)
            .collect();
        let first = std::panic::catch_unwind(|| codec.decode(&input, &limits));
        let second = std::panic::catch_unwind(|| codec.decode(&input, &limits));
        assert!(first.is_ok());
        assert!(second.is_ok());
        assert_eq!(first.unwrap(), second.unwrap());
    }
}

#[test]
fn every_truncation_of_valid_output_is_rejected() {
    let codec = BuiltinCodec;
    let bytes = codec.encode(&sample(), &Limits::default()).unwrap();
    for end in 0..bytes.len() {
        assert!(
            codec.decode(&bytes[..end], &Limits::default()).is_err(),
            "offset {end}"
        );
    }
}

#[test]
fn input_and_structural_limits_are_enforced_before_large_work() {
    let codec = BuiltinCodec;
    let input_limits = Limits::new(
        31,
        1024,
        10,
        10,
        10,
        10,
        10,
        64 * 1024 * 1024,
        64 * 1024 * 1024,
        7,
    );
    let error = codec.decode(&[0; 32], &input_limits).unwrap_err();
    assert!(
        matches!(error, EngineError::LimitExceeded { context, .. } if context == "input bytes")
    );

    let mut bytes = Vec::from(*RSLD_MAGIC);
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    let structural_limits = Limits::new(
        1024,
        1024,
        10,
        1,
        1,
        10,
        10,
        64 * 1024 * 1024,
        64 * 1024 * 1024,
        7,
    );
    let error = codec.decode(&bytes, &structural_limits).unwrap_err();
    assert!(
        matches!(error, EngineError::LimitExceeded { context, .. } if context == "shapes per slide")
    );
}

#[test]
fn unsupported_real_pptx_header_is_not_misidentified_as_supported() {
    let adapter = PptxAdapter::default();
    assert!(adapter.load(&[0x50, 0x4b, 0x03, 0x04]).is_err());
}

#[test]
fn unsupported_objects_are_reported_as_capability_gaps() {
    let mut slide = Slide::new(SlideId::new(1));
    slide
        .add_object(SlideObject::new(
            ObjectId::new(2),
            ObjectKind::Shape(Shape {
                kind: ShapeKind::RoundedRectangle,
                ..Default::default()
            }),
            Rect::new(slides_api::Point::ZERO, 10.0, 10.0),
            Transform::IDENTITY,
        ))
        .unwrap();
    let mut presentation = Presentation::new(PresentationId::new(1));
    presentation.add_slide(slide).unwrap();
    assert!(matches!(
        BuiltinCodec.encode(&presentation, &Limits::default()),
        Err(EngineError::UnsupportedCapability(_))
    ));
}

fn flattened_text(presentation: &Presentation) -> String {
    let mut out = String::new();
    for slide in &presentation.slides {
        for object in &slide.objects {
            if let ObjectKind::Text(text) = &object.kind {
                for paragraph in &text.paragraphs {
                    for run in &paragraph.runs {
                        out.push_str(&run.text);
                    }
                }
            }
        }
    }
    out
}

#[test]
fn office_oxide_engine_works_behind_trait_object() {
    let engine: &dyn PresentationEngine = &office_oxide_adapter();
    let bytes = PresentationEngine::save(engine, &sample()).unwrap();
    assert!(bytes.starts_with(b"PK\x03\x04"));
    assert!(bytes.len() > 100);

    let decoded = PresentationEngine::load(engine, &bytes).unwrap();
    assert_eq!(decoded.slides.len(), 1);
    assert!(flattened_text(&decoded).contains("integration"));
    assert!(engine.supported_capabilities().contains(&Capability::Text));
    assert!(
        engine
            .supported_capabilities()
            .contains(&Capability::Images)
    );
}

#[test]
fn public_synthetic_fixture_round_trips_supported_text_and_png() {
    const SUPPORTED: &[u8] =
        include_bytes!("../../../../fixtures/public/slides/supported-roundtrip.pptx");

    let engine: &dyn PresentationEngine = &office_oxide_adapter();
    let decoded = PresentationEngine::load(engine, SUPPORTED).unwrap();
    assert_eq!(decoded.slides.len(), 2);
    assert!(flattened_text(&decoded).contains("PUBLIC-SYNTHETIC-01"));
    assert!(flattened_text(&decoded).contains("PUBLIC-SYNTHETIC-02"));

    let first_text = decoded.slides[0]
        .objects
        .iter()
        .find(|object| matches!(&object.kind, ObjectKind::Text(_)))
        .expect("first slide text object");
    assert!(first_text.bounds.width > 0.0);
    assert!(first_text.bounds.height > 0.0);

    let image = decoded.slides[1]
        .objects
        .iter()
        .find_map(|object| match &object.kind {
            ObjectKind::Image(image) => Some(image),
            _ => None,
        })
        .expect("second slide synthetic PNG");
    assert_eq!(image.format, slides_api::SlideImageFormat::Png);
    assert!(image.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    let encoded = PresentationEngine::save(engine, &decoded).unwrap();
    assert!(encoded.starts_with(b"PK\x03\x04"));
    let round_tripped = PresentationEngine::load(engine, &encoded).unwrap();
    assert_eq!(round_tripped.slides.len(), 2);
    assert!(flattened_text(&round_tripped).contains("PUBLIC-SYNTHETIC-01"));
    assert!(flattened_text(&round_tripped).contains("PUBLIC-SYNTHETIC-02"));
    let round_tripped_image = round_tripped.slides[1]
        .objects
        .iter()
        .find_map(|object| match &object.kind {
            ObjectKind::Image(image) => Some(image),
            _ => None,
        })
        .expect("round-tripped synthetic PNG");
    assert_eq!(round_tripped_image.bytes, image.bytes);
}

#[test]
fn office_oxide_with_limits_is_public_and_delegates_defaults() {
    let explicit = office_oxide_adapter_with_limits(Limits::default());
    let implicit = office_oxide_adapter();
    assert_eq!(explicit.limits(), implicit.limits());
    assert_eq!(explicit.capabilities(), implicit.capabilities());
}

#[test]
fn office_oxide_hostile_bytes_do_not_unwind_through_public_load() {
    let engine: &dyn PresentationEngine = &office_oxide_adapter();
    for seed in 0_u16..=255 {
        let length = usize::from(seed % 97);
        let input: Vec<u8> = (0..length)
            .map(|index| (u16::try_from(index).unwrap() ^ seed) as u8)
            .collect();
        let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PresentationEngine::load(engine, &input)
        }));
        let second = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PresentationEngine::load(engine, &input)
        }));
        assert!(first.is_ok(), "seed {seed} unwound");
        assert!(second.is_ok(), "seed {seed} unwound");
        assert_eq!(
            first.unwrap(),
            second.unwrap(),
            "seed {seed} not deterministic"
        );
    }
}

#[test]
fn office_oxide_every_truncation_is_rejected_without_unwind() {
    let engine: &dyn PresentationEngine = &office_oxide_adapter();
    let bytes = PresentationEngine::save(engine, &sample()).unwrap();
    for end in 0..bytes.len() {
        let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PresentationEngine::load(engine, &bytes[..end])
        }));
        let second = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PresentationEngine::load(engine, &bytes[..end])
        }));
        assert!(first.is_ok(), "offset {end} unwound");
        assert!(second.is_ok(), "offset {end} unwound");
        let first_inner = first.unwrap();
        let second_inner = second.unwrap();
        assert_eq!(first_inner, second_inner, "offset {end} not deterministic");
        assert!(first_inner.is_err(), "offset {end} unexpectedly decoded");
    }
}
