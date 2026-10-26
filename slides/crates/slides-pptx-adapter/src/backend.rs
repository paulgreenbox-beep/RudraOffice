use slides_api::{Capability, EngineError, Presentation, PresentationEngine};

use crate::{BuiltinCodec, Limits};

/// Extension point for a format-specific implementation.
///
/// Implementations must be deterministic for equal inputs and must enforce the
/// supplied limits before allocating from untrusted input. This trait performs
/// no filesystem or network I/O; it operates only on byte slices and DTOs.
pub trait PresentationCodec {
    fn capabilities(&self) -> &[Capability];

    fn decode(&self, bytes: &[u8], limits: &Limits) -> Result<Presentation, EngineError>;

    fn encode(&self, document: &Presentation, limits: &Limits) -> Result<Vec<u8>, EngineError>;
}

/// Backend extension point used by [`PptxAdapter`].
///
/// A backend may wrap a real PPTX codec or another bounded representation. The
/// adapter owns no UI or editor/core state and never performs external I/O.
pub trait PresentationBackend {
    fn capabilities(&self) -> &[Capability];

    fn limits(&self) -> &Limits;

    fn load_bytes(&self, bytes: &[u8]) -> Result<Presentation, EngineError>;

    fn save_bytes(&self, document: &Presentation) -> Result<Vec<u8>, EngineError>;
}

/// A backend that supplies limits to a codec implementation.
pub struct CodecBackend<C> {
    codec: C,
    limits: Limits,
}

impl<C> CodecBackend<C> {
    pub fn new(codec: C, limits: Limits) -> Self {
        Self { codec, limits }
    }

    pub fn codec(&self) -> &C {
        &self.codec
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }
}

impl<C: PresentationCodec> PresentationBackend for CodecBackend<C> {
    fn capabilities(&self) -> &[Capability] {
        self.codec.capabilities()
    }

    fn limits(&self) -> &Limits {
        &self.limits
    }

    fn load_bytes(&self, bytes: &[u8]) -> Result<Presentation, EngineError> {
        self.codec.decode(bytes, &self.limits)
    }

    fn save_bytes(&self, document: &Presentation) -> Result<Vec<u8>, EngineError> {
        self.codec.encode(document, &self.limits)
    }
}

/// Default bounded adapter façade.
pub struct PptxAdapter<B = CodecBackend<BuiltinCodec>> {
    backend: B,
}

impl Default for PptxAdapter<CodecBackend<BuiltinCodec>> {
    fn default() -> Self {
        Self::new()
    }
}

impl PptxAdapter<CodecBackend<BuiltinCodec>> {
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        Self {
            backend: CodecBackend::new(BuiltinCodec, limits),
        }
    }
}

impl<C: PresentationCodec> PptxAdapter<CodecBackend<C>> {
    pub fn with_codec(codec: C, limits: Limits) -> Self {
        Self {
            backend: CodecBackend::new(codec, limits),
        }
    }
}

impl<B: PresentationBackend> PptxAdapter<B> {
    pub fn with_backend(backend: B) -> Self {
        Self { backend }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn limits(&self) -> &Limits {
        self.backend.limits()
    }

    pub fn capabilities(&self) -> &[Capability] {
        self.backend.capabilities()
    }

    pub fn load_bytes(&self, bytes: &[u8]) -> Result<Presentation, EngineError> {
        self.backend.load_bytes(bytes)
    }

    pub fn save_bytes(&self, document: &Presentation) -> Result<Vec<u8>, EngineError> {
        self.backend.save_bytes(document)
    }
}

impl<B: PresentationBackend> PresentationEngine for PptxAdapter<B> {
    fn load(&self, bytes: &[u8]) -> Result<Presentation, EngineError> {
        self.load_bytes(bytes)
    }

    fn save(&self, document: &Presentation) -> Result<Vec<u8>, EngineError> {
        self.save_bytes(document)
    }

    fn capabilities(&self) -> &[Capability] {
        self.capabilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slides_api::{PresentationId, Slide};

    #[test]
    fn façade_delegates_to_injected_backend() {
        #[derive(Default)]
        struct TestBackend;
        static CAPS: [Capability; 1] = [Capability::LoadSave];
        static LIMITS: Limits = Limits::new(
            16 * 1024 * 1024,
            16 * 1024 * 1024,
            1024,
            4096,
            1_000_000,
            1024 * 1024,
            8 * 1024 * 1024,
            64 * 1024 * 1024,
            64 * 1024 * 1024,
            7,
        );

        impl PresentationBackend for TestBackend {
            fn capabilities(&self) -> &[Capability] {
                &CAPS
            }

            fn limits(&self) -> &Limits {
                &LIMITS
            }

            fn load_bytes(&self, _bytes: &[u8]) -> Result<Presentation, EngineError> {
                Ok(Presentation::new(PresentationId::new(1)))
            }

            fn save_bytes(&self, _document: &Presentation) -> Result<Vec<u8>, EngineError> {
                Ok(vec![7])
            }
        }

        let adapter = PptxAdapter::with_backend(TestBackend);
        assert_eq!(adapter.capabilities(), &[Capability::LoadSave]);
        assert_eq!(adapter.limits(), &LIMITS);
        assert_eq!(adapter.load_bytes(&[1]).unwrap().slides.len(), 0);
        let mut document = Presentation::new(PresentationId::new(1));
        document
            .slides
            .push(Slide::new(slides_api::SlideId::new(1)));
        assert_eq!(adapter.save_bytes(&document).unwrap(), vec![7]);
    }
}
