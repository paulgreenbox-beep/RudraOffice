//! Bounded presentation adapters.
//!
//! [`BuiltinCodec`] remains available for deterministic compatibility tests.
//! [`office_oxide_adapter`] is the production path: it reads and writes real
//! OOXML/PPTX packages through the locally vendored `office_oxide` engine.

mod backend;
mod builtin_codec;
mod limits;
mod office_oxide_codec;

pub use backend::{CodecBackend, PptxAdapter, PresentationBackend, PresentationCodec};
pub use builtin_codec::{BuiltinCodec, RSLD_MAGIC};
pub use limits::{Limits, MAX_RECORD_REMAINING_BYTES, MIN_DOCUMENT_BYTES};
pub use office_oxide_codec::OfficeOxideCodec;

/// A presentation adapter backed by the real OOXML/PPTX codec.
pub type OfficeOxidePptxAdapter = PptxAdapter<CodecBackend<OfficeOxideCodec>>;

/// Create the production PPTX adapter with explicit resource limits.
pub fn office_oxide_adapter_with_limits(limits: Limits) -> OfficeOxidePptxAdapter {
    PptxAdapter::with_codec(OfficeOxideCodec, limits)
}

/// Create the production PPTX adapter with default resource limits.
pub fn office_oxide_adapter() -> OfficeOxidePptxAdapter {
    office_oxide_adapter_with_limits(Limits::default())
}
