use std::collections::BTreeSet;

use slides_api::{
    Background, Capability, DEFAULT_SLIDE_SIZE, EngineError, ObjectId, ObjectKind, Paragraph,
    ParagraphFormatting, Presentation, PresentationId, Rect, Shape, ShapeKind, Slide, SlideId,
    SlideLayout, SlideObject, Text, TextFormatting, Transform,
};

use crate::{Limits, PresentationCodec};

pub const RSLD_MAGIC: &[u8; 7] = b"RSLD001";
const TAG_TEXT: u8 = 1;
const TAG_RECTANGLE: u8 = 2;
const TAG_ELLIPSE: u8 = 3;

static CAPABILITIES: [Capability; 4] = [
    Capability::LoadSave,
    Capability::Slides,
    Capability::Shapes,
    Capability::Text,
];

/// Deterministic built-in codec for the bounded `RSLD001` representation.
///
/// This is deliberately not an OOXML/PPTX codec. The supported subset contains
/// slide records and text, rectangle, and ellipse objects. Slide geometry is
/// represented by object bounds; every decoded slide supplies the API default
/// size and no notes. Values use fixed little-endian integers, records have no
/// optional fields, and trailing bytes are rejected, making output stable and
/// malformed input observable.
///
/// The codec rejects unsupported model states rather than silently dropping
/// them: non-identity transforms, non-default slide metadata, non-default
/// shape styling, multi-paragraph text, and rich-text formatting are all
/// rejected at encode time so that round trips are lossless for the supported
/// subset.
#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltinCodec;

impl PresentationCodec for BuiltinCodec {
    fn capabilities(&self) -> &[Capability] {
        &CAPABILITIES
    }

    fn decode(&self, bytes: &[u8], limits: &Limits) -> Result<Presentation, EngineError> {
        limits.validate()?;
        limits.check_input_len(bytes.len())?;
        let mut reader = Reader::new(bytes);
        if reader.take(7)? != RSLD_MAGIC {
            return Err(EngineError::InvalidFormat);
        }
        limits.check_remaining_bytes(
            reader.bytes.len() - reader.offset,
            limits.min_record_remaining_bytes,
        )?;

        let presentation_id = reader.u64()?;
        validate_id(presentation_id, "presentation")?;
        let slide_count = reader.u32()?;
        limits.check_slides(slide_count)?;
        let remaining_after_header = reader.bytes.len() - reader.offset;
        limits.check_remaining_bytes(remaining_after_header, limits.min_record_remaining_bytes)?;
        let mut slides = Vec::new();
        let mut slide_ids = BTreeSet::new();
        let mut total_shapes = 0_u64;
        let mut total_text_bytes = 0_u64;

        for _ in 0..slide_count {
            let slide_id = reader.u64()?;
            let shape_count = reader.u32()?;
            validate_id(slide_id, "slide")?;
            ensure_unique_id(&mut slide_ids, slide_id, "slide")?;
            limits.check_shapes_per_slide(shape_count)?;
            limits.check_total_shapes(total_shapes.saturating_add(u64::from(shape_count)))?;
            let mut objects = Vec::new();
            let mut object_ids = BTreeSet::new();

            for _ in 0..shape_count {
                let tag = reader.u8()?;
                let object_id = reader.u64()?;
                let x = reader.f32()?;
                let y = reader.f32()?;
                let width = reader.f32()?;
                let height = reader.f32()?;
                validate_id(object_id, "object")?;
                ensure_unique_id(&mut object_ids, object_id, "object")?;
                if !x.is_finite()
                    || !y.is_finite()
                    || !width.is_finite()
                    || !height.is_finite()
                    || width < 0.0
                    || height < 0.0
                {
                    return Err(EngineError::InvalidInput(
                        "object geometry must be finite and non-negative in size".to_string(),
                    ));
                }
                total_shapes = total_shapes
                    .checked_add(1)
                    .ok_or_else(|| EngineError::CodecError("shape count overflow".to_string()))?;
                limits.check_total_shapes(total_shapes)?;

                let kind = match tag {
                    TAG_TEXT => {
                        let text_len = reader.u32()?;
                        let text_len_usize = usize::try_from(text_len).map_err(|_| {
                            EngineError::CodecError("text length does not fit usize".to_string())
                        })?;
                        limits.check_text_len(text_len_usize)?;
                        let remaining_before_text = reader.bytes.len() - reader.offset;
                        if text_len_usize > remaining_before_text {
                            return Err(EngineError::CodecError(
                                "truncated text payload".to_string(),
                            ));
                        }
                        total_text_bytes = total_text_bytes
                            .checked_add(u64::from(text_len))
                            .ok_or_else(|| {
                                EngineError::CodecError("text length overflow".to_string())
                            })?;
                        limits.check_total_text(total_text_bytes)?;
                        let text = String::from_utf8(reader.take(text_len_usize)?.to_vec())
                            .map_err(|_| EngineError::Utf8Error)?;
                        ObjectKind::Text(Text {
                            paragraphs: vec![Paragraph::plain(text)],
                        })
                    }
                    TAG_RECTANGLE => ObjectKind::Shape(Shape {
                        kind: ShapeKind::Rectangle,
                        ..Default::default()
                    }),
                    TAG_ELLIPSE => ObjectKind::Shape(Shape {
                        kind: ShapeKind::Ellipse,
                        ..Default::default()
                    }),
                    _ => {
                        return Err(EngineError::CodecError(format!(
                            "unknown shape tag {}",
                            tag
                        )));
                    }
                };
                objects.push(SlideObject::new(
                    ObjectId::new(object_id),
                    kind,
                    Rect::new(slides_api::Point::new(x, y), width, height),
                    Transform::IDENTITY,
                ));
            }
            slides.push(Slide {
                id: SlideId::new(slide_id),
                objects,
                ..Slide::default()
            });
        }

        if !reader.is_empty() {
            return Err(EngineError::CodecError(
                "trailing bytes after final record".to_string(),
            ));
        }
        let presentation = Presentation {
            id: PresentationId::new(presentation_id),
            slides,
        };
        presentation
            .validate()
            .map_err(EngineError::ValidationError)?;
        Ok(presentation)
    }

    fn encode(&self, document: &Presentation, limits: &Limits) -> Result<Vec<u8>, EngineError> {
        limits.validate()?;
        let (slide_count, total_shapes, total_text_bytes) = validate_document(document, limits)?;
        let encoded_len = encoded_len(document, limits)?;
        limits.check_output_len(encoded_len)?;
        let capacity = usize::try_from(encoded_len)
            .map_err(|_| EngineError::CodecError("output length does not fit usize".to_string()))?;
        let mut output = Vec::with_capacity(capacity);
        output.extend_from_slice(RSLD_MAGIC);
        put_u64(&mut output, document.id.get());
        put_u32(&mut output, slide_count);
        for slide in &document.slides {
            validate_slide_metadata(slide)?;
            put_u64(&mut output, slide.id.get());
            put_u32(
                &mut output,
                u32::try_from(slide.objects.len()).map_err(|_| EngineError::LimitExceeded {
                    limit: u64::from(limits.max_shapes_per_slide),
                    actual: slide.objects.len() as u64,
                    context: "shapes per slide".to_string(),
                })?,
            );
            for object in &slide.objects {
                validate_object_transform(object)?;
                let (tag, text) = match &object.kind {
                    ObjectKind::Text(text) => {
                        let text = flatten_text(text, limits)?;
                        (TAG_TEXT, Some(text))
                    }
                    ObjectKind::Shape(shape) => {
                        validate_shape_style(shape)?;
                        match shape.kind {
                            ShapeKind::Rectangle => (TAG_RECTANGLE, None),
                            ShapeKind::Ellipse => (TAG_ELLIPSE, None),
                            ShapeKind::RoundedRectangle => {
                                return Err(EngineError::UnsupportedCapability(
                                    "rounded rectangles".to_string(),
                                ));
                            }
                            ShapeKind::Chevron => {
                                return Err(EngineError::UnsupportedCapability(
                                    "chevrons".to_string(),
                                ));
                            }
                        }
                    }
                    other => {
                        return Err(EngineError::UnsupportedCapability(other.name().to_string()));
                    }
                };
                output.push(tag);
                put_u64(&mut output, object.id.get());
                put_f32(&mut output, object.bounds.origin.x);
                put_f32(&mut output, object.bounds.origin.y);
                put_f32(&mut output, object.bounds.width);
                put_f32(&mut output, object.bounds.height);
                if let Some(text) = text {
                    limits.check_text_len(text.len())?;
                    put_u32(
                        &mut output,
                        u32::try_from(text.len()).map_err(|_| EngineError::LimitExceeded {
                            limit: u64::from(limits.max_text_bytes),
                            actual: text.len() as u64,
                            context: "text bytes".to_string(),
                        })?,
                    );
                    output.extend_from_slice(text.as_bytes());
                }
            }
        }
        debug_assert_eq!(output.len() as u64, encoded_len);
        debug_assert_eq!(
            total_shapes,
            document
                .slides
                .iter()
                .map(|slide| slide.objects.len() as u64)
                .sum::<u64>()
        );
        debug_assert_eq!(total_text_bytes, text_bytes(document, limits)?);
        Ok(output)
    }
}

fn flatten_text(text: &Text, limits: &Limits) -> Result<String, EngineError> {
    if text.paragraphs.len() != 1 {
        return Err(EngineError::UnsupportedCapability(
            "multi-paragraph text".to_string(),
        ));
    }
    let paragraph = &text.paragraphs[0];
    if paragraph.runs.len() != 1 {
        return Err(EngineError::UnsupportedCapability(
            "multi-run text".to_string(),
        ));
    }
    if paragraph.formatting != ParagraphFormatting::default() {
        return Err(EngineError::UnsupportedCapability(
            "paragraph formatting".to_string(),
        ));
    }
    let run = &paragraph.runs[0];
    if run.formatting != TextFormatting::default() {
        return Err(EngineError::UnsupportedCapability(
            "text run formatting".to_string(),
        ));
    }
    let result = run.text.clone();
    limits.check_text_len(result.len())?;
    Ok(result)
}

fn validate_shape_style(shape: &Shape) -> Result<(), EngineError> {
    if shape.fill.is_some() || shape.stroke.is_some() || shape.stroke_width != 0.0 {
        return Err(EngineError::UnsupportedCapability(
            "shape styling".to_string(),
        ));
    }
    Ok(())
}

fn validate_slide_metadata(slide: &Slide) -> Result<(), EngineError> {
    if slide.size != DEFAULT_SLIDE_SIZE {
        return Err(EngineError::UnsupportedCapability(
            "non-default slide size".to_string(),
        ));
    }
    if slide.notes.is_some() {
        return Err(EngineError::UnsupportedCapability(
            "speaker notes".to_string(),
        ));
    }
    if slide.layout != SlideLayout::Blank {
        return Err(EngineError::UnsupportedCapability(
            "slide layout".to_string(),
        ));
    }
    if slide.background != Background::None {
        return Err(EngineError::UnsupportedCapability(
            "slide background".to_string(),
        ));
    }
    if slide.transition.is_some() {
        return Err(EngineError::UnsupportedCapability(
            "slide transition".to_string(),
        ));
    }
    Ok(())
}

fn validate_object_transform(object: &SlideObject) -> Result<(), EngineError> {
    if object.transform != Transform::IDENTITY {
        return Err(EngineError::UnsupportedCapability(
            "non-identity transform (translation, scale, or rotation)".to_string(),
        ));
    }
    Ok(())
}

fn validate_document(
    document: &Presentation,
    limits: &Limits,
) -> Result<(u32, u64, u64), EngineError> {
    let slide_count =
        u32::try_from(document.slides.len()).map_err(|_| EngineError::LimitExceeded {
            limit: u64::from(limits.max_slides),
            actual: document.slides.len() as u64,
            context: "slides".to_string(),
        })?;
    limits.check_slides(slide_count)?;
    let mut total_shapes = 0_u64;
    let mut total_text_bytes = 0_u64;
    for slide in &document.slides {
        let shapes =
            u32::try_from(slide.objects.len()).map_err(|_| EngineError::LimitExceeded {
                limit: u64::from(limits.max_shapes_per_slide),
                actual: slide.objects.len() as u64,
                context: "shapes per slide".to_string(),
            })?;
        limits.check_shapes_per_slide(shapes)?;
        total_shapes = total_shapes
            .checked_add(u64::from(shapes))
            .ok_or_else(|| EngineError::CodecError("shape count overflow".to_string()))?;
        limits.check_total_shapes(total_shapes)?;
        for object in &slide.objects {
            if let ObjectKind::Text(text) = &object.kind {
                let text = flatten_text(text, limits)?;
                limits.check_text_len(text.len())?;
                total_text_bytes = total_text_bytes
                    .checked_add(text.len() as u64)
                    .ok_or_else(|| EngineError::CodecError("text length overflow".to_string()))?;
                limits.check_total_text(total_text_bytes)?;
            } else if !matches!(
                object.kind,
                ObjectKind::Shape(slides_api::Shape {
                    kind: ShapeKind::Rectangle | ShapeKind::Ellipse,
                    ..
                })
            ) {
                return Err(EngineError::UnsupportedCapability(
                    object.kind.name().to_string(),
                ));
            }
        }
    }
    document.validate().map_err(EngineError::ValidationError)?;
    Ok((slide_count, total_shapes, total_text_bytes))
}

fn text_bytes(document: &Presentation, limits: &Limits) -> Result<u64, EngineError> {
    document
        .slides
        .iter()
        .flat_map(|slide| slide.objects.iter())
        .filter_map(|object| match &object.kind {
            ObjectKind::Text(text) => Some(text),
            _ => None,
        })
        .try_fold(0_u64, |total, text| {
            let text_len = text
                .paragraphs
                .first()
                .and_then(|p| p.runs.first().map(|r| r.text.len()))
                .unwrap_or(0);
            limits.check_text_len(text_len)?;
            total
                .checked_add(text_len as u64)
                .ok_or_else(|| EngineError::CodecError("text length overflow".to_string()))
        })
}

fn encoded_len(document: &Presentation, limits: &Limits) -> Result<u64, EngineError> {
    let mut length = 7_u64
        .checked_add(8)
        .and_then(|value| value.checked_add(4))
        .ok_or_else(|| overflow("header"))?;
    for slide in &document.slides {
        length = length
            .checked_add(8)
            .and_then(|value| value.checked_add(4))
            .ok_or_else(|| overflow("slide header"))?;
        for object in &slide.objects {
            length = length
                .checked_add(1 + 8 + 16)
                .ok_or_else(|| overflow("object record"))?;
            if let ObjectKind::Text(text) = &object.kind {
                let text_len = text
                    .paragraphs
                    .first()
                    .and_then(|p| p.runs.first().map(|r| r.text.len()))
                    .unwrap_or(0);
                limits.check_text_len(text_len)?;
                length = length
                    .checked_add(4)
                    .and_then(|value| value.checked_add(text_len as u64))
                    .ok_or_else(|| overflow("text bytes"))?;
            }
        }
    }
    Ok(length)
}

fn overflow(context: &'static str) -> EngineError {
    EngineError::CodecError(format!("encoded length overflow in {}", context))
}

fn validate_id(id: u64, kind: &'static str) -> Result<(), EngineError> {
    if id == 0 {
        return Err(EngineError::ReferenceIntegrity(format!(
            "{} id must be non-zero",
            kind
        )));
    }
    Ok(())
}

fn ensure_unique_id(
    ids: &mut BTreeSet<u64>,
    id: u64,
    kind: &'static str,
) -> Result<(), EngineError> {
    if !ids.insert(id) {
        return Err(EngineError::ReferenceIntegrity(format!(
            "duplicate {} id {}",
            kind, id
        )));
    }
    Ok(())
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_f32(output: &mut Vec<u8>, value: f32) {
    output.extend_from_slice(&value.to_bits().to_le_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], EngineError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| EngineError::CodecError("reader offset overflow".to_string()))?;
        let result = self.bytes.get(self.offset..end).ok_or_else(|| {
            EngineError::CodecError(format!(
                "truncated input at byte {}, need {} bytes",
                self.offset, length
            ))
        })?;
        self.offset = end;
        Ok(result)
    }

    fn u8(&mut self) -> Result<u8, EngineError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, EngineError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, EngineError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn f32(&mut self) -> Result<f32, EngineError> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MIN_DOCUMENT_BYTES;
    use crate::PptxAdapter;
    use slides_api::{ObjectKind, PresentationEngine, Shape, TextRun};

    fn sample() -> Presentation {
        let mut slide = Slide::new(SlideId::new(1));
        slide
            .add_object(SlideObject::new(
                ObjectId::new(2),
                ObjectKind::Text(Text {
                    paragraphs: vec![Paragraph::plain("Hello, мир")],
                }),
                Rect::new(slides_api::Point::ZERO, 300.0, 80.0),
                Transform::IDENTITY,
            ))
            .unwrap();
        slide
            .add_object(SlideObject::new(
                ObjectId::new(3),
                ObjectKind::Shape(Shape {
                    kind: ShapeKind::Rectangle,
                    ..Default::default()
                }),
                Rect::new(slides_api::Point::new(10.0, 20.0), 100.0, 50.0),
                Transform::IDENTITY,
            ))
            .unwrap();
        slide
            .add_object(SlideObject::new(
                ObjectId::new(4),
                ObjectKind::Shape(Shape {
                    kind: ShapeKind::Ellipse,
                    ..Default::default()
                }),
                Rect::new(slides_api::Point::new(30.0, 40.0), 60.0, 40.0),
                Transform::IDENTITY,
            ))
            .unwrap();
        let mut presentation = Presentation::new(PresentationId::new(10));
        presentation.add_slide(slide).unwrap();
        presentation
    }

    #[test]
    fn round_trip_preserves_supported_subset() {
        let codec = BuiltinCodec;
        let bytes = codec.encode(&sample(), &Limits::default()).unwrap();
        let decoded = codec.decode(&bytes, &Limits::default()).unwrap();
        assert_eq!(decoded, sample());
        assert_eq!(decoded.slides[0].size, DEFAULT_SLIDE_SIZE);
        assert_eq!(decoded.slides[0].notes, None);
        assert_eq!(decoded.slides[0].objects[0].transform, Transform::IDENTITY);
    }

    #[test]
    fn output_is_deterministic() {
        let codec = BuiltinCodec;
        let limits = Limits::default();
        assert_eq!(
            codec.encode(&sample(), &limits).unwrap(),
            codec.encode(&sample(), &limits).unwrap()
        );
    }

    #[test]
    fn malformed_inputs_are_rejected_without_panicking() {
        let codec = BuiltinCodec;
        for length in 0..=64 {
            let input = vec![0xA5; length];
            assert!(codec.decode(&input, &Limits::default()).is_err());
        }
    }

    #[test]
    fn oversized_input_is_rejected_before_header_parse() {
        let codec = BuiltinCodec;
        let input = codec.encode(&sample(), &Limits::default()).unwrap();
        assert!(input.len() > MIN_DOCUMENT_BYTES as usize);
        let limits = Limits::new(
            MIN_DOCUMENT_BYTES,
            100,
            10,
            10,
            10,
            10,
            10,
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            7,
        );
        let error = BuiltinCodec.decode(&input, &limits).unwrap_err();
        assert!(matches!(error, EngineError::LimitExceeded { .. }));
    }

    #[test]
    fn truncated_valid_prefix_is_rejected() {
        let codec = BuiltinCodec;
        let bytes = codec.encode(&sample(), &Limits::default()).unwrap();
        for end in 0..bytes.len() {
            assert!(codec.decode(&bytes[..end], &Limits::default()).is_err());
        }
    }

    #[test]
    fn invalid_utf8_is_rejected() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 10);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        bytes.push(TAG_TEXT);
        put_u64(&mut bytes, 2);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 10.0);
        put_f32(&mut bytes, 10.0);
        put_u32(&mut bytes, 1);
        bytes.push(0xff);
        assert_eq!(
            BuiltinCodec.decode(&bytes, &Limits::default()),
            Err(EngineError::Utf8Error)
        );
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let mut document = sample();
        document.slides[0].objects[1].id = ObjectId::new(2);
        assert!(matches!(
            BuiltinCodec.encode(&document, &Limits::default()),
            Err(EngineError::ValidationError(_))
        ));
    }

    #[test]
    fn output_limit_is_checked_before_allocation() {
        let limits = Limits::new(
            1000,
            MIN_DOCUMENT_BYTES,
            10,
            10,
            10,
            100,
            100,
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            7,
        );
        assert!(matches!(
            BuiltinCodec.encode(&sample(), &limits),
            Err(EngineError::LimitExceeded { context, .. }) if context == "output bytes"
        ));
    }

    #[test]
    fn invalid_limits_are_rejected_for_encode_and_decode() {
        let invalid_configs = [
            Limits::new(101, 100, 1, 0, 0, 0, 0, 100, 100, 7),
            Limits::new(100, 101, 1, 0, 0, 0, 0, 100, 100, 7),
            Limits::new(0, 100, 1, 0, 0, 0, 0, 100, 100, 7),
            Limits::new(100, 0, 1, 0, 0, 0, 0, 100, 100, 7),
            Limits::new(100, 100, 1, 0, 0, 0, 0, 0, 100, 7),
            Limits::new(100, 100, 1, 0, 0, 0, 0, 100, 0, 7),
            Limits::new(100, 100, 0, 0, 0, 0, 0, 100, 100, 7),
            Limits::new(100, 100, 1, 0, 0, 0, 0, 100, 100, 6),
            Limits::new(100, 100, 1, 0, 0, 0, 0, 100, 100, 13),
            Limits::new(30, 100, 1, 0, 0, 0, 0, 100, 100, 7),
            Limits::new(100, 30, 1, 0, 0, 0, 0, 100, 100, 7),
        ];
        for limits in invalid_configs {
            assert!(matches!(
                limits.validate(),
                Err(EngineError::InvalidInput(_))
            ));
            assert!(matches!(
                BuiltinCodec.encode(&sample(), &limits),
                Err(EngineError::InvalidInput(_))
            ));
            assert!(matches!(
                BuiltinCodec.decode(RSLD_MAGIC, &limits),
                Err(EngineError::InvalidInput(_))
            ));
        }
    }

    #[test]
    fn paragraph_runs_are_flattened_deterministically() {
        let text = Text {
            paragraphs: vec![Paragraph {
                runs: vec![TextRun::new("a"), TextRun::new("b")],
                ..Paragraph::default()
            }],
        };
        let limits = Limits::default();
        assert!(matches!(
            flatten_text(&text, &limits),
            Err(EngineError::UnsupportedCapability(_))
        ));
    }

    #[test]
    fn encode_rejects_non_identity_transform() {
        let mut document = sample();
        document.slides[0].objects[0].transform =
            Transform::new(slides_api::Point::new(1.0, 0.0), 1.0);
        assert!(matches!(
            BuiltinCodec.encode(&document, &Limits::default()),
            Err(EngineError::UnsupportedCapability(_))
        ));
    }

    #[test]
    fn encode_rejects_rotation_even_when_translation_and_scale_are_identity() {
        let mut document = sample();
        document.slides[0].objects[0].transform.rotation_degrees = 45.0;
        assert!(matches!(
            BuiltinCodec.encode(&document, &Limits::default()),
            Err(EngineError::UnsupportedCapability(capability)) if capability.contains("rotation")
        ));
    }

    #[test]
    fn encode_rejects_non_default_slide_size() {
        let mut document = sample();
        document.slides[0].size = slides_api::Size::new(1024.0, 768.0);
        assert!(matches!(
            BuiltinCodec.encode(&document, &Limits::default()),
            Err(EngineError::UnsupportedCapability(capability)) if capability == "non-default slide size"
        ));
    }

    #[test]
    fn encode_rejects_speaker_notes() {
        let mut document = sample();
        document.slides[0].notes = Some("Presenter cue".to_string());
        assert!(matches!(
            BuiltinCodec.encode(&document, &Limits::default()),
            Err(EngineError::UnsupportedCapability(capability)) if capability == "speaker notes"
        ));
    }

    #[test]
    fn encode_rejects_non_default_shape_styling() {
        let mut document = sample();
        document.slides[0].objects[1].kind = ObjectKind::Shape(Shape {
            kind: ShapeKind::Rectangle,
            fill: Some(slides_api::Color::BLACK),
            ..Default::default()
        });
        assert!(matches!(
            BuiltinCodec.encode(&document, &Limits::default()),
            Err(EngineError::UnsupportedCapability(_))
        ));
    }

    #[test]
    fn encode_rejects_non_blank_slide_layout() {
        let mut document = sample();
        document.slides[0].layout = SlideLayout::Title;
        assert!(matches!(
            BuiltinCodec.encode(&document, &Limits::default()),
            Err(EngineError::UnsupportedCapability(_))
        ));
    }

    #[test]
    fn encode_rejects_non_default_slide_background() {
        let mut document = sample();
        document.slides[0].background = Background::Solid(slides_api::Color::WHITE);
        assert!(matches!(
            BuiltinCodec.encode(&document, &Limits::default()),
            Err(EngineError::UnsupportedCapability(_))
        ));
    }

    #[test]
    fn encode_rejects_multi_paragraph_text() {
        let mut document = sample();
        document.slides[0].objects[0].kind = ObjectKind::Text(Text {
            paragraphs: vec![Paragraph::plain("first"), Paragraph::plain("second")],
        });
        assert!(matches!(
            BuiltinCodec.encode(&document, &Limits::default()),
            Err(EngineError::UnsupportedCapability(_))
        ));
    }

    #[test]
    fn encode_rejects_rich_text_runs() {
        let mut document = sample();
        document.slides[0].objects[0].kind = ObjectKind::Text(Text {
            paragraphs: vec![Paragraph {
                runs: vec![TextRun::new("a"), TextRun::new("b")],
                ..Paragraph::default()
            }],
        });
        assert!(matches!(
            BuiltinCodec.encode(&document, &Limits::default()),
            Err(EngineError::UnsupportedCapability(_))
        ));
    }

    #[test]
    fn round_trip_preserves_empty_presentation_rejected_by_api() {
        let codec = BuiltinCodec;
        let empty = Presentation::new(PresentationId::new(1));
        assert!(matches!(
            codec.encode(&empty, &Limits::default()),
            Err(EngineError::ValidationError(_))
        ));
    }

    #[test]
    fn count_bomb_with_large_slide_count_is_rejected() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, u32::MAX);
        let limits = Limits::new(
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            1024,
            4096,
            1_000_000,
            1024 * 1024,
            8 * 1024 * 1024,
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            7,
        );
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &limits),
            Err(EngineError::LimitExceeded { context, .. }) if context == "slides"
        ));
    }

    #[test]
    fn count_bomb_with_large_shape_count_is_rejected() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, u32::MAX);
        let limits = Limits::new(
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            1024,
            10,
            1_000_000,
            1024 * 1024,
            8 * 1024 * 1024,
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            7,
        );
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &limits),
            Err(EngineError::LimitExceeded { context, .. }) if context == "shapes per slide"
        ));
    }

    #[test]
    fn count_bomb_with_large_text_length_is_rejected() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        bytes.push(TAG_TEXT);
        put_u64(&mut bytes, 2);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 10.0);
        put_f32(&mut bytes, 10.0);
        put_u32(&mut bytes, u32::MAX);
        let limits = Limits::new(
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            1024,
            4096,
            1_000_000,
            1024,
            8 * 1024 * 1024,
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            7,
        );
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &limits),
            Err(EngineError::LimitExceeded { context, .. }) if context == "text bytes"
        ));
    }

    #[test]
    fn decode_rejects_zero_presentation_id() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 0);
        put_u32(&mut bytes, 0);
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &Limits::default()),
            Err(EngineError::ReferenceIntegrity(_))
        ));
    }

    #[test]
    fn decode_rejects_zero_slide_id() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 0);
        put_u32(&mut bytes, 0);
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &Limits::default()),
            Err(EngineError::ReferenceIntegrity(_))
        ));
    }

    #[test]
    fn decode_rejects_zero_object_id() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        bytes.push(TAG_TEXT);
        put_u64(&mut bytes, 0);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 10.0);
        put_f32(&mut bytes, 10.0);
        put_u32(&mut bytes, 1);
        bytes.push(b'x');
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &Limits::default()),
            Err(EngineError::ReferenceIntegrity(_))
        ));
    }

    #[test]
    fn decode_rejects_duplicate_slide_ids() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 2);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 0);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 0);
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &Limits::default()),
            Err(EngineError::ReferenceIntegrity(_))
        ));
    }

    #[test]
    fn decode_rejects_duplicate_object_ids() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 2);
        bytes.push(TAG_TEXT);
        put_u64(&mut bytes, 2);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 10.0);
        put_f32(&mut bytes, 10.0);
        put_u32(&mut bytes, 1);
        bytes.push(b'x');
        bytes.push(TAG_TEXT);
        put_u64(&mut bytes, 2);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 10.0);
        put_f32(&mut bytes, 10.0);
        put_u32(&mut bytes, 1);
        bytes.push(b'y');
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &Limits::default()),
            Err(EngineError::ReferenceIntegrity(_))
        ));
    }

    #[test]
    fn decode_rejects_large_text_length_by_limit() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        bytes.push(TAG_TEXT);
        put_u64(&mut bytes, 2);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 10.0);
        put_f32(&mut bytes, 10.0);
        put_u32(&mut bytes, 2000);
        bytes.extend(std::iter::repeat_n(b'a', 2000));
        let limits = Limits::new(
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            1024,
            4096,
            1_000_000,
            1024,
            8 * 1024 * 1024,
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            7,
        );
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &limits),
            Err(EngineError::LimitExceeded { context, .. }) if context == "text bytes"
        ));
    }

    #[test]
    fn decode_rejects_truncated_text_payload() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        bytes.push(TAG_TEXT);
        put_u64(&mut bytes, 2);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 10.0);
        put_f32(&mut bytes, 10.0);
        put_u32(&mut bytes, 100);
        bytes.extend(std::iter::repeat_n(b'a', 50));
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &Limits::default()),
            Err(EngineError::CodecError(_))
        ));
    }

    #[test]
    fn decode_rejects_negative_or_nan_geometry() {
        for bad_x in [f32::NAN, f32::NEG_INFINITY] {
            let mut bytes = Vec::from(*RSLD_MAGIC);
            put_u64(&mut bytes, 1);
            put_u32(&mut bytes, 1);
            put_u64(&mut bytes, 1);
            put_u32(&mut bytes, 1);
            bytes.push(TAG_RECTANGLE);
            put_u64(&mut bytes, 2);
            put_f32(&mut bytes, bad_x);
            put_f32(&mut bytes, 0.0);
            put_f32(&mut bytes, 10.0);
            put_f32(&mut bytes, 10.0);
            assert!(
                BuiltinCodec.decode(&bytes, &Limits::default()).is_err(),
                "bad_x={bad_x}"
            );
        }
        for bad_wh in [f32::NEG_INFINITY, -1.0] {
            let mut bytes = Vec::from(*RSLD_MAGIC);
            put_u64(&mut bytes, 1);
            put_u32(&mut bytes, 1);
            put_u64(&mut bytes, 1);
            put_u32(&mut bytes, 1);
            bytes.push(TAG_RECTANGLE);
            put_u64(&mut bytes, 2);
            put_f32(&mut bytes, 0.0);
            put_f32(&mut bytes, 0.0);
            put_f32(&mut bytes, bad_wh);
            put_f32(&mut bytes, 10.0);
            assert!(
                BuiltinCodec.decode(&bytes, &Limits::default()).is_err(),
                "bad_width={bad_wh}"
            );
            let mut bytes = Vec::from(*RSLD_MAGIC);
            put_u64(&mut bytes, 1);
            put_u32(&mut bytes, 1);
            put_u64(&mut bytes, 1);
            put_u32(&mut bytes, 1);
            bytes.push(TAG_RECTANGLE);
            put_u64(&mut bytes, 2);
            put_f32(&mut bytes, 0.0);
            put_f32(&mut bytes, 0.0);
            put_f32(&mut bytes, 10.0);
            put_f32(&mut bytes, bad_wh);
            assert!(
                BuiltinCodec.decode(&bytes, &Limits::default()).is_err(),
                "bad_height={bad_wh}"
            );
        }
    }

    #[test]
    fn decode_rejects_negative_width_or_height() {
        for bad_wh in [f32::NEG_INFINITY, -1.0] {
            let mut bytes = Vec::from(*RSLD_MAGIC);
            put_u64(&mut bytes, 1);
            put_u32(&mut bytes, 1);
            put_u64(&mut bytes, 1);
            put_u32(&mut bytes, 1);
            bytes.push(TAG_RECTANGLE);
            put_u64(&mut bytes, 2);
            put_f32(&mut bytes, 0.0);
            put_f32(&mut bytes, 0.0);
            put_f32(&mut bytes, bad_wh);
            put_f32(&mut bytes, 10.0);
            assert!(
                BuiltinCodec.decode(&bytes, &Limits::default()).is_err(),
                "bad_wh={bad_wh}"
            );
            let mut bytes = Vec::from(*RSLD_MAGIC);
            put_u64(&mut bytes, 1);
            put_u32(&mut bytes, 1);
            put_u64(&mut bytes, 1);
            put_u32(&mut bytes, 1);
            bytes.push(TAG_RECTANGLE);
            put_u64(&mut bytes, 2);
            put_f32(&mut bytes, 0.0);
            put_f32(&mut bytes, 0.0);
            put_f32(&mut bytes, 10.0);
            put_f32(&mut bytes, bad_wh);
            assert!(
                BuiltinCodec.decode(&bytes, &Limits::default()).is_err(),
                "bad_wh={bad_wh}"
            );
        }
    }

    #[test]
    fn decode_rejects_unknown_shape_tag() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        bytes.push(0xff);
        put_u64(&mut bytes, 2);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 0.0);
        put_f32(&mut bytes, 10.0);
        put_f32(&mut bytes, 10.0);
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &Limits::default()),
            Err(EngineError::CodecError(_))
        ));
    }

    #[test]
    fn decode_rejects_text_length_overflow() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        bytes.push(TAG_TEXT);
        put_u64(&mut bytes, 2);
        put_u32(&mut bytes, u32::MAX);
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &Limits::default()),
            Err(EngineError::CodecError(_))
        ));
    }

    #[test]
    fn decode_rejects_trailing_bytes_after_valid_record() {
        let mut bytes = Vec::from(*RSLD_MAGIC);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 1);
        put_u64(&mut bytes, 1);
        put_u32(&mut bytes, 0);
        bytes.push(0xAA);
        assert!(matches!(
            BuiltinCodec.decode(&bytes, &Limits::default()),
            Err(EngineError::CodecError(_))
        ));
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
        let oversized_input = codec.encode(&sample(), &Limits::default()).unwrap();
        assert!(oversized_input.len() > MIN_DOCUMENT_BYTES as usize);
        let input_limits = Limits::new(
            MIN_DOCUMENT_BYTES,
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
        let error = BuiltinCodec
            .decode(&oversized_input, &input_limits)
            .unwrap_err();
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
}
