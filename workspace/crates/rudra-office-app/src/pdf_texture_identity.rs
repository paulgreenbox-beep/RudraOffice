//! Pure identity predicate extracted from the PDF GPU texture cache.
//! Document/page types stay owned by their existing backend; this is not a
//! replacement for the public application DocumentId or session registry.

pub(crate) fn matches_pdf_texture<D: Eq, P: Eq>(
    cached_document: D,
    cached_page: P,
    cached_scale: u32,
    frame_document: D,
    frame_page: P,
    frame_scale: u32,
) -> bool {
    cached_document == frame_document && cached_page == frame_page && cached_scale == frame_scale
}

#[cfg(test)]
mod tests {
    use super::matches_pdf_texture;

    #[test]
    fn different_documents_cannot_reuse_page_one_at_the_same_scale() {
        assert!(!matches_pdf_texture(
            1_u64,
            1_u64,
            1_f32.to_bits(),
            2_u64,
            1_u64,
            1_f32.to_bits()
        ));
    }

    #[test]
    fn identical_document_page_and_scale_can_reuse_texture() {
        assert!(matches_pdf_texture(
            7_u64,
            3_u64,
            1_f32.to_bits(),
            7_u64,
            3_u64,
            1_f32.to_bits()
        ));
    }

    #[test]
    fn different_page_or_scale_requires_fresh_texture() {
        assert!(!matches_pdf_texture(
            7_u64,
            1_u64,
            1_f32.to_bits(),
            7_u64,
            2_u64,
            1_f32.to_bits()
        ));
        assert!(!matches_pdf_texture(
            7_u64,
            1_u64,
            1_f32.to_bits(),
            7_u64,
            1_u64,
            2_f32.to_bits()
        ));
    }

    #[test]
    fn reopening_with_a_new_backend_id_does_not_resurrect_old_pixels() {
        assert!(!matches_pdf_texture(
            41_u64,
            1_u64,
            0.75_f32.to_bits(),
            42_u64,
            1_u64,
            0.75_f32.to_bits()
        ));
    }
}
