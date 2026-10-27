use pdf_core::{DocumentId, PageId, PdfEngine};
use pdf_oxide_adapter::{PdfOpenArtifact, PdfOxideAdapter, PdfViewerSafetyHint};

fn tiny_pdf(page_one_attributes: &str, catalog_extra: &str) -> (Vec<u8>, usize) {
    let content = b"q 1 0 0 rg 20 20 30 20 re f Q\n";
    tiny_pdf_with_streams(page_one_attributes, catalog_extra, content, content)
}

fn tiny_pdf_with_streams(
    page_one_attributes: &str,
    catalog_extra: &str,
    first_content: &[u8],
    second_content: &[u8],
) -> (Vec<u8>, usize) {
    let stream = |content: &[u8]| {
        format!(
            "<< /Length {} >>\nstream\n{}endstream",
            content.len(),
            std::str::from_utf8(content).unwrap()
        )
    };
    let objects = [
        format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>"),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R {page_one_attributes} /Resources << >> /Contents 5 0 R >>"
        ),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /Contents 6 0 R >>"
            .to_owned(),
        stream(first_content),
        stream(second_content),
    ];

    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0usize];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref_offset = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    (bytes, xref_offset)
}

fn normal_pdf() -> Vec<u8> {
    tiny_pdf(
        "/MediaBox [0 0 120 80] /CropBox [10 5 110 75] /Rotate -270",
        "",
    )
    .0
}

#[test]
fn dense_page_budget_does_not_reject_the_document_or_a_supported_sibling() {
    // Reduced vector-density trigger: a dense page followed by a simple page.
    // The whole file remains below admission; only the render input is refused.
    let command = b"0 0 m 1 1 l S\n";
    let dense = command.repeat(8 * 1024 * 1024 / command.len() + 1);
    let bytes = tiny_pdf_with_streams(
        "/MediaBox [0 0 120 80]",
        "",
        &dense,
        b"q 1 0 0 rg 20 20 30 20 re f Q\n",
    )
    .0;
    let mut engine = PdfOxideAdapter::default();
    let document = engine.parse(&bytes).expect("admit and parse both pages");
    assert_eq!(document.page_count, 2);
    let error = engine
        .render_page(document.document_id, PageId::new(1), 1.0)
        .unwrap_err();
    assert!(error.to_string().contains("страница 1"));
    assert!(error.to_string().contains("content budget"));
    let job_error = engine
        .prepare_render(
            pdf_oxide_adapter::PdfRenderRequest::new(document.document_id, PageId::new(1), 1.0, 1)
                .unwrap(),
        )
        .unwrap()
        .render()
        .unwrap_err();
    assert_eq!(
        job_error, error,
        "worker job must keep the same decoded-content guard"
    );
    let png = engine
        .render_page(document.document_id, PageId::new(2), 1.0)
        .unwrap();
    assert_eq!(png_dimensions(&png), (200, 100));
    let job_png = engine
        .prepare_render(
            pdf_oxide_adapter::PdfRenderRequest::new(document.document_id, PageId::new(2), 1.0, 2)
                .unwrap(),
        )
        .unwrap()
        .render()
        .unwrap();
    let pixels = |bytes: &[u8]| {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8()
    };
    assert_eq!(pixels(&job_png), pixels(&png));
    assert_eq!(engine.document_count(), 1);
    engine.close_document(document.document_id).unwrap();
    assert_eq!(engine.document_count(), 0);
}

fn png_dimensions(png: &[u8]) -> (u32, u32) {
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(&png[12..16], b"IHDR");
    (
        u32::from_be_bytes(png[16..20].try_into().unwrap()),
        u32::from_be_bytes(png[20..24].try_into().unwrap()),
    )
}

fn assert_cid_word_spacing_renders_identically(trigger: &[u8], control: &[u8]) -> image::RgbaImage {
    let mut engine = PdfOxideAdapter::default();
    let first = engine.parse(trigger).unwrap();
    let second = engine.parse(control).unwrap();
    assert_eq!(first.page_count, 1);
    assert_eq!(second.page_count, 1);
    let render = |engine: &mut PdfOxideAdapter, id| {
        let png = engine.render_page(id, PageId::new(1), 1.0).unwrap();
        image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8()
    };
    let with_tw = render(&mut engine, first.document_id);
    let without_tw = render(&mut engine, second.document_id);
    assert_eq!(with_tw.dimensions(), without_tw.dimensions());
    assert!(without_tw.pixels().any(|pixel| pixel[0] < 100));
    assert!(
        with_tw.as_raw() == without_tw.as_raw(),
        "Tw must not change two-byte CID text merely because ToUnicode maps a CID to SPACE"
    );
    without_tw
}

#[test]
fn two_byte_cid_unicode_spaces_do_not_receive_word_spacing() {
    // These original, synthetic fixtures use Type0 /Identity-H and CID 0003
    // mapped to U+0020. The handcrafted embedded font has only a Mac cmap,
    // so the intended direct CID-to-GID render path is exercised. PDF word
    // spacing applies to single-byte source code 32, not that Unicode mapping.
    let trigger = include_bytes!("../../../../fixtures/public/pdf/cid-unicode-space-tw.pdf");
    let control = include_bytes!("../../../../fixtures/public/pdf/cid-unicode-space-control.pdf");
    for bytes in [trigger.as_slice(), control.as_slice()] {
        for token in [
            b"/Encoding /Identity-H".as_slice(),
            b"/CIDToGIDMap /Identity".as_slice(),
            b"<0003> <0020>".as_slice(),
            b"<000100030002> Tj".as_slice(),
        ] {
            assert!(bytes.windows(token.len()).any(|window| window == token));
        }
    }
    assert_eq!(trigger.len(), control.len());
    assert_eq!(
        trigger
            .iter()
            .zip(control.iter())
            .filter(|(a, b)| a != b)
            .count(),
        1,
        "the two files must differ only in the Tw operand"
    );
    assert!(trigger
        .windows(b"40.000 Tw".len())
        .any(|w| w == b"40.000 Tw"));
    assert!(control
        .windows(b"00.000 Tw".len())
        .any(|w| w == b"00.000 Tw"));

    let image = assert_cid_word_spacing_renders_identically(trigger, control);
    assert_eq!(image.dimensions(), (200, 100));
    let dark_in = |left: u32, right: u32| {
        image.enumerate_pixels().any(|(x, _, pixel)| {
            (left..right).contains(&x)
                && pixel[0] < 100
                && pixel[1] < 100
                && pixel[2] < 100
                && pixel[3] > 200
        })
    };
    assert!(
        dark_in(19, 31),
        "the synthetic A before the space must be visible"
    );
    assert!(
        dark_in(44, 56),
        "the synthetic B after the space must be visible"
    );
}

#[test]
fn public_synthetic_cid_preserves_non_ascii_unicode() {
    let bytes = include_bytes!("../../../../fixtures/public/pdf/cid-cyrillic.pdf");
    let backend = pdf_oxide::PdfDocument::from_bytes(bytes.to_vec()).unwrap();
    let text = backend.extract_text(0).unwrap();
    assert_eq!(text.split_whitespace().collect::<Vec<_>>().join(" "), "А Б");
    let mut adapter = PdfOxideAdapter::default();
    assert_eq!(adapter.parse(bytes).unwrap().page_count, 1);
}

#[test]
#[ignore = "requires private reduced PDFs in RUDRA_PDF_PRIVATE_WORD_SPACING_DIR"]
fn private_cid_unicode_space_regression() {
    // Retain the original source-derived case for an explicit local-only run.
    // The private PDFs and the font they contain are never embedded or shipped.
    let directory = std::path::PathBuf::from(
        std::env::var_os("RUDRA_PDF_PRIVATE_WORD_SPACING_DIR")
            .expect("set RUDRA_PDF_PRIVATE_WORD_SPACING_DIR for this ignored test"),
    );
    let trigger = std::fs::read(directory.join("cid-unicode-space-tw.pdf"))
        .expect("read private trigger PDF");
    let control = std::fs::read(directory.join("cid-unicode-space-control.pdf"))
        .expect("read private control PDF");
    let _ = assert_cid_word_spacing_renders_identically(&trigger, &control);
}

#[test]
fn detached_artifact_is_send_and_real_adoption_renders_png() {
    fn assert_send<T: Send>() {}
    assert_send::<PdfOpenArtifact>();

    let mut engine = PdfOxideAdapter::default();
    let artifact = engine.parse_detached(&normal_pdf()).unwrap();
    let provisional = artifact.document_info();
    assert_eq!(provisional.document_id, DocumentId::new(0));
    assert_eq!(provisional.page_count, 2);
    assert_eq!(provisional.page_ids, vec![PageId::new(1), PageId::new(2)]);
    assert!(artifact.initial_previews().is_empty());
    assert!(artifact.safety_hints().is_empty());
    assert_eq!(engine.document_count(), 0);
    assert!(engine.last_open_safety_hints().is_empty());

    let adopted = engine.adopt_detached(artifact).unwrap();
    assert_eq!(adopted.document_id, DocumentId::new(1));
    assert_eq!(engine.document_count(), 1);
    let geometry = engine
        .page_geometry(adopted.document_id, PageId::new(1))
        .unwrap();
    assert_eq!(geometry.media_box, [0.0, 0.0, 120.0, 80.0]);
    assert_eq!(geometry.crop_box, Some([10.0, 5.0, 110.0, 75.0]));
    assert_eq!(geometry.rotation_degrees, 90);
    assert_eq!(
        (geometry.width_points, geometry.height_points),
        (70.0, 100.0)
    );

    let png = engine
        .render_page(adopted.document_id, PageId::new(2), 1.0)
        .unwrap();
    assert_eq!(png_dimensions(&png), (200, 100));
    let pixels = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .unwrap()
        .to_rgba8();
    assert!(
        pixels
            .pixels()
            .any(|pixel| pixel[0] > 180 && pixel[1] < 100 && pixel[2] < 100),
        "the real content stream should paint red pixels"
    );
}

#[test]
fn preparing_invalid_jobs_keeps_the_registered_document_and_valid_page_usable() {
    use pdf_oxide_adapter::PdfRenderRequest;

    let mut engine = PdfOxideAdapter::default();
    let document = engine.parse(&normal_pdf()).unwrap();
    let invalid_page = PdfRenderRequest::new(document.document_id, PageId::new(3), 1.0, 1).unwrap();
    assert!(engine.prepare_render(invalid_page).is_err());
    let unknown_document =
        PdfRenderRequest::new(DocumentId::new(99), PageId::new(1), 1.0, 2).unwrap();
    assert!(engine.prepare_render(unknown_document).is_err());
    assert_eq!(engine.document_count(), 1);
    let png = engine
        .prepare_render(
            PdfRenderRequest::new(document.document_id, PageId::new(2), 1.0, 3).unwrap(),
        )
        .unwrap()
        .render()
        .unwrap();
    assert_eq!(png_dimensions(&png), (200, 100));
    engine.close_document(document.document_id).unwrap();
    assert_eq!(engine.document_count(), 0);
}

#[test]
fn direct_parse_and_two_document_close_keep_ids_isolated() {
    let mut engine = PdfOxideAdapter::default();
    let first = engine.parse(&normal_pdf()).unwrap();
    let second = engine
        .adopt_detached(engine.parse_detached(&normal_pdf()).unwrap())
        .unwrap();
    assert_ne!(first.document_id, second.document_id);
    assert_eq!(engine.document_count(), 2);
    assert!(engine
        .page_geometry(first.document_id, PageId::new(0))
        .is_err());
    assert!(engine
        .render_page(first.document_id, PageId::new(3), 1.0)
        .is_err());

    engine.close_document(first.document_id).unwrap();
    assert_eq!(engine.document_count(), 1);
    assert!(engine
        .page_geometry(first.document_id, PageId::new(1))
        .unwrap_err()
        .to_string()
        .contains("unknown PDF document ID"));
    assert!(engine.close_document(first.document_id).is_err());
    assert_eq!(
        png_dimensions(
            &engine
                .render_page(second.document_id, PageId::new(2), 1.0)
                .unwrap()
        ),
        (200, 100)
    );
    engine.close_document(second.document_id).unwrap();
    assert_eq!(engine.document_count(), 0);

    let third = engine.parse(&normal_pdf()).unwrap();
    assert!(third.document_id.value() > second.document_id.value());
}

#[test]
fn safety_notices_are_bounded_nonblocking_and_reset_on_direct_failure() {
    let (bytes, _) = tiny_pdf(
        "/MediaBox [0 0 120 80]",
        "/OpenAction << /S /Java#53cript /JS (ignored) >> /X /Launch /Y /URI /Z /EmbeddedFile /Q /Filespec /R /GoToR /T /EF",
    );
    let mut engine = PdfOxideAdapter::default();
    let artifact = engine.parse_detached(&bytes).unwrap();
    let hints = artifact.safety_hints();
    assert_eq!(hints.len(), 7);
    for expected in [
        PdfViewerSafetyHint::JavaScript,
        PdfViewerSafetyHint::Launch,
        PdfViewerSafetyHint::ExternalUrl,
        PdfViewerSafetyHint::EmbeddedFile,
        PdfViewerSafetyHint::FileSpecification,
        PdfViewerSafetyHint::GoToRemote,
        PdfViewerSafetyHint::OpenAction,
    ] {
        assert!(hints.contains(&expected), "missing {expected:?}");
    }
    assert_eq!(engine.document_count(), 0);
    assert!(engine.last_open_safety_hints().is_empty());
    let info = engine.adopt_detached(artifact).unwrap();
    assert_eq!(engine.last_open_safety_hints(), hints);
    assert!(engine
        .render_page(info.document_id, PageId::new(1), 1.0)
        .is_ok());

    assert!(engine.parse(b"not a PDF").is_err());
    assert!(engine.last_open_safety_hints().is_empty());
    assert_eq!(engine.document_count(), 1);
    engine.parse(&normal_pdf()).unwrap();
    assert!(engine.last_open_safety_hints().is_empty());
}

#[test]
fn incremental_revisions_are_accepted_but_final_tail_is_controlled() {
    let (mut bytes, previous_xref) = tiny_pdf("/MediaBox [0 0 120 80]", "");
    let object_offset = bytes.len();
    bytes.extend_from_slice(b"7 0 obj\n<< /Producer (second revision) >>\nendobj\n");
    let revision_xref = bytes.len();
    bytes.extend_from_slice(
        format!(
            "xref\n7 1\n{object_offset:010} 00000 n \ntrailer\n<< /Size 8 /Root 1 0 R /Info 7 0 R /Prev {previous_xref} >>\nstartxref\n{revision_xref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    let mut engine = PdfOxideAdapter::default();
    let info = engine.parse(&bytes).unwrap();
    assert_eq!(info.page_count, 2);

    let mut trailing = bytes.clone();
    trailing.extend_from_slice(b"\r\n \t");
    assert!(engine.parse_detached(&trailing).is_ok());
    trailing.extend_from_slice(b"payload");
    let error = engine.parse_detached(&trailing).err().unwrap().to_string();
    assert!(
        error.contains("non-whitespace data follows final %%EOF"),
        "{error}"
    );
    assert_eq!(engine.document_count(), 1);
}

#[test]
fn crop_is_intersected_and_bad_page_boxes_fail_without_losing_other_pages() {
    let (bytes, _) = tiny_pdf(
        "/MediaBox [0 0 120 80] /CropBox [-20 -10 100 60] /Rotate -90",
        "",
    );
    let mut engine = PdfOxideAdapter::default();
    let doc = engine.parse(&bytes).unwrap();
    let geometry = engine
        .page_geometry(doc.document_id, PageId::new(1))
        .unwrap();
    assert_eq!(geometry.crop_box, Some([0.0, 0.0, 100.0, 60.0]));
    assert_eq!(geometry.rotation_degrees, 270);
    assert_eq!(
        (geometry.width_points, geometry.height_points),
        (60.0, 100.0)
    );
    let clipped_png = engine
        .render_page(doc.document_id, PageId::new(1), 1.0)
        .unwrap();
    assert_eq!(png_dimensions(&clipped_png), (60, 100));

    for bad_attributes in [
        "/MediaBox [0 0 0 80]",
        "/MediaBox [0 0 120 80] /CropBox [200 200 250 250]",
    ] {
        let (bytes, _) = tiny_pdf(bad_attributes, "");
        let bad = engine.parse(&bytes).unwrap();
        assert!(engine
            .page_geometry(bad.document_id, PageId::new(1))
            .is_err());
        assert!(engine
            .render_page(bad.document_id, PageId::new(1), 1.0)
            .is_err());
        assert!(engine
            .render_page(bad.document_id, PageId::new(2), 1.0)
            .is_ok());
    }
}

#[test]
fn rendered_crop_matches_geometry_at_all_quarter_turns() {
    let mut engine = PdfOxideAdapter::default();
    for (rotation, rectangle) in [
        (0, (20, 30, 60, 40)),
        (90, (10, 20, 40, 60)),
        (180, (40, 10, 60, 40)),
        (270, (30, 40, 40, 60)),
    ] {
        let (full_pdf, _) = tiny_pdf(&format!("/MediaBox [0 0 120 80] /Rotate {rotation}"), "");
        let (cropped_pdf, _) = tiny_pdf(
            &format!("/MediaBox [0 0 120 80] /CropBox [20 10 80 50] /Rotate {rotation}"),
            "",
        );
        let full_id = engine.parse(&full_pdf).unwrap().document_id;
        let cropped_id = engine.parse(&cropped_pdf).unwrap().document_id;
        let full_png = engine.render_page(full_id, PageId::new(1), 1.0).unwrap();
        let cropped_png = engine.render_page(cropped_id, PageId::new(1), 1.0).unwrap();
        let full_image = image::load_from_memory_with_format(&full_png, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        let cropped_image =
            image::load_from_memory_with_format(&cropped_png, image::ImageFormat::Png)
                .unwrap()
                .to_rgba8();
        let (x, y, width, height) = rectangle;
        assert_eq!(cropped_image.dimensions(), (width, height));
        assert_eq!(
            cropped_image,
            image::imageops::crop_imm(&full_image, x, y, width, height).to_image(),
            "rotation {rotation} crop must match the corresponding full raster region"
        );
    }
}

#[test]
fn invalid_zoom_and_raster_budgets_fail_before_render() {
    let mut engine = PdfOxideAdapter::default();
    let document_id = engine.parse(&normal_pdf()).unwrap().document_id;
    for scale in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -1.0, 0.001] {
        let err = engine
            .render_page(document_id, PageId::new(1), scale)
            .unwrap_err();
        assert!(err.to_string().contains("render scale"), "{err}");
    }
    let byte_err = engine
        .render_page(document_id, PageId::new(1), 40.0)
        .unwrap_err();
    assert!(
        byte_err.to_string().contains("raster byte budget"),
        "{byte_err}"
    );
    let crop_err = engine
        .render_page(document_id, PageId::new(1), 30.0)
        .unwrap_err();
    assert!(
        crop_err.to_string().contains("crop working byte budget"),
        "{crop_err}"
    );
    let pixel_err = engine
        .render_page(document_id, PageId::new(1), 50.0)
        .unwrap_err();
    assert!(
        pixel_err.to_string().contains("pixel budget"),
        "{pixel_err}"
    );
    assert!(engine.render_page(document_id, PageId::new(1), 1.0).is_ok());
}

#[test]
#[ignore = "set RUDRA_PDF_SMOKE_PATH to a read-only acceptance PDF"]
fn smoke_real_pdf_parse_and_first_page_png() {
    let path = std::env::var_os("RUDRA_PDF_SMOKE_PATH").expect("set RUDRA_PDF_SMOKE_PATH");
    let bytes = std::fs::read(&path).expect("read source PDF without modifying it");
    let mut engine = PdfOxideAdapter::default();
    let artifact = engine.parse_detached(&bytes).expect("PDFOxide parse");
    let info = engine.adopt_detached(artifact).expect("adopt parsed PDF");
    let geometry = engine
        .page_geometry(info.document_id, PageId::new(1))
        .expect("first page geometry");
    let png = engine
        .render_page(info.document_id, PageId::new(1), 0.5)
        .expect("first page render");
    let dimensions = png_dimensions(&png);
    println!(
        "source_bytes={} pages={} first_page_points={}x{} png_bytes={} png_dimensions={}x{}",
        bytes.len(),
        info.page_count,
        geometry.width_points,
        geometry.height_points,
        png.len(),
        dimensions.0,
        dimensions.1
    );
    engine.close_document(info.document_id).unwrap();
    assert_eq!(engine.document_count(), 0);
}
