use std::fmt::Debug;
use std::sync::mpsc;
use std::time::Duration;

use pdf_core::{DocumentId, PageId, PdfEngine};
use pdf_oxide_adapter::{
    PdfOpenArtifact, PdfOxideAdapter, PdfRenderCompletion, PdfRenderJob, PdfRenderRequest,
    PdfRenderWorker,
};

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

fn test_pdf() -> Vec<u8> {
    tiny_pdf("/MediaBox [0 0 80 60]", "").0
}

fn request(document_id: DocumentId, request_id: u64) -> PdfRenderRequest {
    PdfRenderRequest::new(document_id, PageId::new(1), 1.0, request_id)
        .expect("valid render request")
}

fn rgba(png: &[u8]) -> image::RgbaImage {
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .expect("renderer returns a decodable PNG")
        .to_rgba8()
}

fn assert_same_pixels(expected_png: &[u8], actual_png: &[u8]) {
    let expected = rgba(expected_png);
    let actual = rgba(actual_png);
    assert_eq!(actual.dimensions(), expected.dimensions());
    assert_eq!(actual, expected, "every rendered RGBA pixel should match");
}

#[test]
fn public_render_types_are_send_and_request_keeps_its_identity() {
    fn assert_send<T: Send>() {}
    fn assert_copy_eq_debug<T: Copy + Eq + Debug>() {}
    fn assert_debug_send<T: Debug + Send>() {}

    assert_send::<PdfOpenArtifact>();
    assert_send::<PdfOxideAdapter>();
    assert_send::<PdfRenderRequest>();
    assert_send::<PdfRenderCompletion>();
    assert_send::<PdfRenderWorker>();
    assert_copy_eq_debug::<PdfRenderRequest>();
    assert_debug_send::<PdfRenderJob>();

    let document_id = DocumentId::new(17);
    let page_id = PageId::new(3);
    let request = PdfRenderRequest::new(document_id, page_id, 1.25, 9).unwrap();
    let copied = request;
    assert_eq!(request, copied);
    assert_eq!(request.document_id(), document_id);
    assert_eq!(request.page_id(), page_id);
    assert_eq!(request.scale(), 1.25);
    assert_eq!(request.scale_bits(), 1.25_f32.to_bits());
    assert_eq!(request.request_id(), 9);
}

#[test]
fn prepared_job_renders_on_a_cpu_thread_with_sync_pixel_equality() {
    let mut adapter = PdfOxideAdapter::default();
    let document_id = adapter.parse(&test_pdf()).unwrap().document_id;
    let request = request(document_id, 1);
    let synchronous_png = adapter
        .render_page(document_id, request.page_id(), request.scale())
        .unwrap();
    let job = adapter.prepare_render(request).unwrap();
    assert_eq!(job.request(), request);

    let worker_png = std::thread::spawn(move || job.render())
        .join()
        .expect("CPU render thread should finish")
        .expect("prepared render should succeed");
    assert_same_pixels(&synchronous_png, &worker_png);
}

#[test]
fn closing_or_dropping_the_adapter_invalidates_prepared_jobs() {
    let mut adapter = PdfOxideAdapter::default();
    let closed_document = adapter.parse(&test_pdf()).unwrap().document_id;
    let closed_job = adapter.prepare_render(request(closed_document, 1)).unwrap();
    adapter.close_document(closed_document).unwrap();
    assert!(closed_job.render().is_err());

    let dropped_document = adapter.parse(&test_pdf()).unwrap().document_id;
    let dropped_job = adapter
        .prepare_render(request(dropped_document, 2))
        .unwrap();
    drop(adapter);
    assert!(dropped_job.render().is_err());
}

#[test]
fn worker_notifier_wakes_for_a_real_png_with_sync_pixel_equality() {
    let mut adapter = PdfOxideAdapter::default();
    let document_id = adapter.parse(&test_pdf()).unwrap().document_id;
    let request = request(document_id, 1);
    let synchronous_png = adapter
        .render_page(document_id, request.page_id(), request.scale())
        .unwrap();
    let job = adapter.prepare_render(request).unwrap();
    let (notified, completion_ready) = mpsc::channel();
    let worker = PdfRenderWorker::new(move || {
        let _ = notified.send(());
    })
    .unwrap();

    worker.submit_latest(job).unwrap();
    completion_ready
        .recv_timeout(Duration::from_secs(5))
        .expect("worker should notify when the render completion is ready");
    let completion = worker
        .take_completion()
        .expect("notified completion should be available");
    assert_eq!(completion.request, request);
    let worker_png = completion.result.unwrap();
    assert_same_pixels(&synchronous_png, &worker_png);
    worker.shutdown();
}

#[test]
fn worker_rejects_invalid_request_ids_scales_and_stale_submissions() {
    assert!(PdfRenderRequest::new(DocumentId::new(1), PageId::new(1), 1.0, 0).is_err());
    for scale in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -1.0, 0.001] {
        assert!(PdfRenderRequest::new(DocumentId::new(1), PageId::new(1), scale, 1).is_err());
    }

    let mut adapter = PdfOxideAdapter::default();
    let document_id = adapter.parse(&test_pdf()).unwrap().document_id;
    let worker = PdfRenderWorker::new(|| {}).unwrap();
    worker
        .submit_latest(adapter.prepare_render(request(document_id, 10)).unwrap())
        .unwrap();
    for stale_id in [10, 9] {
        let stale_job = adapter
            .prepare_render(request(document_id, stale_id))
            .unwrap();
        assert!(worker.submit_latest(stale_job).is_err());
    }
    worker.shutdown();
}

#[test]
fn shutdown_discards_an_available_completion_and_rejects_new_jobs() {
    let mut adapter = PdfOxideAdapter::default();
    let document_id = adapter.parse(&test_pdf()).unwrap().document_id;
    let (notified, completion_ready) = mpsc::channel();
    let worker = PdfRenderWorker::new(move || {
        let _ = notified.send(());
    })
    .unwrap();
    worker
        .submit_latest(adapter.prepare_render(request(document_id, 1)).unwrap())
        .unwrap();
    completion_ready
        .recv_timeout(Duration::from_secs(5))
        .expect("worker should finish before shutdown is checked");

    worker.shutdown();
    assert!(worker.take_completion().is_none());
    let post_shutdown_job = adapter.prepare_render(request(document_id, 2)).unwrap();
    assert!(worker.submit_latest(post_shutdown_job).is_err());
}
