//! Consumer-side guard for raster completions, including errors.
//!
//! The backend request does not contain the runtime session or view generation.
//! Generic types keep the guard testable without a GUI or backend dependency.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PdfRenderIdentity<S, D, P> {
    pub session: S,
    pub backend: D,
    pub page: P,
    pub scale_bits: u32,
    pub sequence: u64,
    pub generation: u64,
}

impl<S: PartialEq, D: PartialEq, P: PartialEq> PdfRenderIdentity<S, D, P> {
    pub(crate) fn matches(&self, actual: &Self) -> bool {
        self.sequence != 0
            && self.generation != 0
            && self.session == actual.session
            && self.backend == actual.backend
            && self.page == actual.page
            && self.scale_bits == actual.scale_bits
            && self.sequence == actual.sequence
            && self.generation == actual.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> PdfRenderIdentity<u64, u64, u64> {
        PdfRenderIdentity {
            session: 1,
            backend: 2,
            page: 8,
            scale_bits: 1.25_f32.to_bits(),
            sequence: 7,
            generation: 3,
        }
    }

    #[test]
    fn every_identity_dimension_gates_pixels_and_errors() {
        let expected = identity();
        assert!(expected.matches(&identity()));
        let changes = [
            PdfRenderIdentity {
                session: 2,
                ..identity()
            },
            PdfRenderIdentity {
                backend: 3,
                ..identity()
            },
            PdfRenderIdentity {
                page: 9,
                ..identity()
            },
            PdfRenderIdentity {
                scale_bits: 1.0_f32.to_bits(),
                ..identity()
            },
            PdfRenderIdentity {
                sequence: 8,
                ..identity()
            },
            PdfRenderIdentity {
                generation: 4,
                ..identity()
            },
        ];
        for changed in changes {
            assert!(!expected.matches(&changed));
        }
    }

    #[test]
    fn zero_ids_and_return_to_the_same_view_never_revive_old_work() {
        let zero_sequence = PdfRenderIdentity {
            sequence: 0,
            ..identity()
        };
        let zero_generation = PdfRenderIdentity {
            generation: 0,
            ..identity()
        };
        assert!(!zero_sequence.matches(&zero_sequence));
        assert!(!zero_generation.matches(&zero_generation));
        let returned = PdfRenderIdentity {
            sequence: 9,
            generation: 5,
            ..identity()
        };
        assert!(!returned.matches(&identity()));
    }
}
