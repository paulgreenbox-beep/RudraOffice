use slides_api::EngineError;

/// Fixed `RSLD001` magic/header prefix length.
const RSLD_MAGIC_HEADER_MIN_BYTES: usize = 7;

/// Minimum operational document byte size: a valid `RSLD001` document
/// occupies at least 7 bytes of magic plus 8 bytes for the presentation id
/// and 4 bytes for the slide count, with 12 additional bytes required for a
/// single empty slide record. We round the per-document minimum up to 31
/// bytes so that implementations can safely assume enough room for the
/// header plus at least one empty slide.
pub const MIN_DOCUMENT_BYTES: u64 = 31;

/// Maximum `min_record_remaining_bytes` the codec will honor. The fixed-size
/// record header consumed before a variable-length payload is 12 bytes, so
/// any value above 12 is a misconfiguration and is rejected at validate time.
pub const MAX_RECORD_REMAINING_BYTES: usize = 12;

/// Resource and structural bounds applied before allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_slides: u32,
    pub max_shapes_per_slide: u32,
    pub max_total_shapes: u64,
    pub max_text_bytes: u32,
    pub max_total_text_bytes: u64,
    /// Hard ceiling on `max_input_bytes`; enforced before any allocation.
    pub max_input_bytes_ceiling: u64,
    /// Hard ceiling on `max_output_bytes`; enforced before any allocation.
    pub max_output_bytes_ceiling: u64,
    /// Minimum remaining bytes required in the input after reading the
    /// fixed-size record header before attempting to parse a variable-length
    /// payload.
    pub min_record_remaining_bytes: usize,
}

impl Limits {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        max_input_bytes: u64,
        max_output_bytes: u64,
        max_slides: u32,
        max_shapes_per_slide: u32,
        max_total_shapes: u64,
        max_text_bytes: u32,
        max_total_text_bytes: u64,
        max_input_bytes_ceiling: u64,
        max_output_bytes_ceiling: u64,
        min_record_remaining_bytes: usize,
    ) -> Self {
        Self {
            max_input_bytes,
            max_output_bytes,
            max_slides,
            max_shapes_per_slide,
            max_total_shapes,
            max_text_bytes,
            max_total_text_bytes,
            max_input_bytes_ceiling,
            max_output_bytes_ceiling,
            min_record_remaining_bytes,
        }
    }

    /// Validates that this configuration can process at least one valid `RSLD001`
    /// document without exceeding its hard resource ceilings.
    ///
    /// Both operational byte maxima must be non-zero and fit beneath their
    /// matching hard ceilings. `max_slides` must be non-zero because the API
    /// rejects empty presentations. The shape and text maxima may be zero:
    /// empty slides are valid documents. The record-remaining threshold must
    /// accommodate the 7-byte `RSLD001` magic/header minimum.
    pub fn validate(&self) -> Result<(), EngineError> {
        validate_operational_byte_limit(
            self.max_input_bytes,
            self.max_input_bytes_ceiling,
            "input bytes",
        )?;
        validate_operational_byte_limit(
            self.max_output_bytes,
            self.max_output_bytes_ceiling,
            "output bytes",
        )?;
        if self.max_input_bytes < MIN_DOCUMENT_BYTES {
            return Err(EngineError::InvalidInput(format!(
                "max_input_bytes must be at least {} bytes to hold the smallest valid RSLD001 document",
                MIN_DOCUMENT_BYTES
            )));
        }
        if self.max_output_bytes < MIN_DOCUMENT_BYTES {
            return Err(EngineError::InvalidInput(format!(
                "max_output_bytes must be at least {} bytes to emit the smallest valid RSLD001 document",
                MIN_DOCUMENT_BYTES
            )));
        }
        if self.max_slides == 0 {
            return Err(EngineError::InvalidInput(
                "max_slides must be non-zero because RSLD001 documents require at least one slide"
                    .to_string(),
            ));
        }
        if self.min_record_remaining_bytes < RSLD_MAGIC_HEADER_MIN_BYTES
            || self.min_record_remaining_bytes > MAX_RECORD_REMAINING_BYTES
        {
            return Err(EngineError::InvalidInput(format!(
                "min_record_remaining_bytes must be between {} and {} bytes inclusive",
                RSLD_MAGIC_HEADER_MIN_BYTES, MAX_RECORD_REMAINING_BYTES
            )));
        }
        Ok(())
    }

    pub(crate) fn check_input_len(&self, actual: usize) -> Result<(), EngineError> {
        check_u64(actual as u64, self.max_input_bytes, "input bytes")
    }

    pub(crate) fn check_output_len(&self, actual: u64) -> Result<(), EngineError> {
        check_u64(actual, self.max_output_bytes, "output bytes")
    }

    pub(crate) fn check_slides(&self, actual: u32) -> Result<(), EngineError> {
        check_u64(actual, u64::from(self.max_slides), "slides")
    }

    pub(crate) fn check_shapes_per_slide(&self, actual: u32) -> Result<(), EngineError> {
        check_u64(
            actual,
            u64::from(self.max_shapes_per_slide),
            "shapes per slide",
        )
    }

    pub(crate) fn check_total_shapes(&self, actual: u64) -> Result<(), EngineError> {
        check_u64(actual, self.max_total_shapes, "total shapes")
    }

    pub(crate) fn check_text_len(&self, actual: usize) -> Result<(), EngineError> {
        check_u64(actual as u64, u64::from(self.max_text_bytes), "text bytes")
    }

    pub(crate) fn check_total_text(&self, actual: u64) -> Result<(), EngineError> {
        check_u64(actual, self.max_total_text_bytes, "total text bytes")
    }

    pub(crate) fn check_remaining_bytes(
        &self,
        actual: usize,
        needed: usize,
    ) -> Result<(), EngineError> {
        if actual < needed {
            return Err(EngineError::CodecError(format!(
                "truncated input: {} remaining bytes, need {} bytes",
                actual, needed
            )));
        }
        Ok(())
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::new(
            512 * 1024 * 1024,
            512 * 1024 * 1024,
            1024,
            4096,
            1_000_000,
            1024 * 1024,
            8 * 1024 * 1024,
            1024 * 1024 * 1024,
            1024 * 1024 * 1024,
            7,
        )
    }
}

fn validate_operational_byte_limit(
    operational: u64,
    ceiling: u64,
    context: &'static str,
) -> Result<(), EngineError> {
    if operational == 0 {
        return Err(EngineError::InvalidInput(format!(
            "configured {context} maximum must be non-zero"
        )));
    }
    if ceiling == 0 {
        return Err(EngineError::InvalidInput(format!(
            "configured {context} ceiling must be non-zero"
        )));
    }
    if operational > ceiling {
        return Err(EngineError::InvalidInput(format!(
            "configured {context} maximum ({operational}) exceeds its ceiling ({ceiling})"
        )));
    }
    Ok(())
}

fn check_u64(actual: impl Into<u64>, limit: u64, context: &'static str) -> Result<(), EngineError> {
    let actual = actual.into();
    if actual > limit {
        return Err(EngineError::LimitExceeded {
            limit,
            actual,
            context: context.to_string(),
        });
    }
    Ok(())
}
