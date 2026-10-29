//! Application routing tests. A real worker supplies PNG bytes; injected late
//! completions exercise the same guard used by the UI event loop, not GUI.
use super::*;
use crate::app_pdf::PDF_VIEW_CACHE_MAX_BYTES;
use std::time::{Duration, Instant};

fn tiny_two_page_pdf() -> Vec<u8> {
    let stream = "q 1 0 0 rg 20 20 30 20 re f Q\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Resources << >> /Contents 5 0 R >>"
            .to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /Contents 6 0 R >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{}endstream",
            stream.len(),
            stream
        ),
        format!(
            "<< /Length {} >>\nstream\n{}endstream",
            stream.len(),
            stream
        ),
    ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0usize];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    bytes
}

fn register_pdf(state: &mut AppState, bytes: &[u8]) -> RuntimeDocumentId {
    let info = state.pdf_engine.parse(bytes).expect("parse synthetic PDF");
    let document = OpenDocument::Pdf {
        document_id: info.document_id,
        page_count: info.page_count,
        page_ids: info.page_ids,
    };
    state
        .register_document(PathBuf::from("synthetic.pdf"), document)
        .unwrap()
}

fn state_with_wake() -> (AppState, Receiver<()>) {
    let (sender, receiver) = mpsc::channel();
    let mut state = AppState::default();
    state.pdf_worker = Some(
        PdfRenderWorker::new(move || {
            let _ = sender.send(());
        })
        .unwrap(),
    );
    (state, receiver)
}

fn current_request(state: &AppState) -> PdfRenderRequest {
    state
        .pdf_raster_batch
        .as_ref()
        .and_then(|batch| batch.request)
        .expect("pending request")
}

fn wait_for_page(state: &mut AppState, wake: &Receiver<()>, page: PageId) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !state
        .pdf_controller
        .state()
        .visible_pages
        .iter()
        .any(|item| item.page_index == page)
    {
        wake.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("bounded actual worker wake");
        state.poll_pdf_raster();
    }
}

#[test]
fn visible_pages_advance_one_at_a_time_with_current_page_first() {
    let (mut state, wake) = state_with_wake();
    let id = register_pdf(&mut state, &tiny_two_page_pdf());
    state.activate_document(id).unwrap();
    assert_eq!(current_request(&state).page_id(), PageId::new(1));
    wait_for_page(&mut state, &wake, PageId::new(1));
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Ready);
    assert_eq!(current_request(&state).page_id(), PageId::new(2));
    wait_for_page(&mut state, &wake, PageId::new(2));
    assert_eq!(state.pdf_controller.state().visible_pages.len(), 2);
    assert!(
        state
            .pdf_controller
            .state()
            .png
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );
}

#[test]
fn cache_hit_invalidates_late_pixels_and_errors_after_zoom_round_trip() {
    let bytes = include_bytes!("../../../../fixtures/public/pdf/cid-unicode-space-control.pdf");
    let (mut state, wake) = state_with_wake();
    let id = register_pdf(&mut state, bytes);
    state.activate_document(id).unwrap();
    wait_for_page(&mut state, &wake, PageId::new(1));
    let original = state.pdf_controller.state().png.clone();
    state.set_zoom(id, 2.0).unwrap();
    let stale = current_request(&state);
    state.set_zoom(id, 1.0).unwrap();
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Ready);
    assert!(!state.adopt_pdf_raster_result(stale, Ok(original.clone())));
    assert!(!state.adopt_pdf_raster_result(stale, Err(pdf_core::PdfError::new("late error"))));
    assert_eq!(state.pdf_controller.state().png, original);
    assert!(state.pdf_error.is_none());
}

#[test]
fn same_batch_cache_hit_cancels_pending_error_and_keeps_cached_pixels() {
    let bytes = include_bytes!("../../../../fixtures/public/pdf/cid-unicode-space-control.pdf");
    let (mut state, _wake) = state_with_wake();
    let id = register_pdf(&mut state, bytes);
    state.activate_document(id).unwrap();
    let pending = current_request(&state);
    // A small ownership sentinel, not PNG/GPU evidence.
    let cached = vec![1, 2, 3];
    state.pdf_controller.cache.insert(
        pending.document_id(),
        pending.page_id(),
        pending.scale(),
        cached.clone(),
    );
    state.pdf_effect = state.pdf_controller.request_current_page();
    state.apply_pdf_effect();
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Ready);
    assert_eq!(state.pdf_controller.state().png, cached);
    assert!(state.pdf_raster_batch.as_ref().unwrap().request.is_none());
    assert!(!state.adopt_pdf_raster_result(pending, Ok(vec![9, 9, 9])));
    assert!(!state.adopt_pdf_raster_result(pending, Err(pdf_core::PdfError::new("late error"))));
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Ready);
    assert_eq!(state.pdf_controller.state().png, cached);
}

#[test]
fn closing_inactive_pdf_keeps_current_session_request_and_rejects_old_completion() {
    let bytes = include_bytes!("../../../../fixtures/public/pdf/cid-unicode-space-control.pdf");
    let (mut state, wake) = state_with_wake();
    let a = register_pdf(&mut state, bytes);
    let b = register_pdf(&mut state, bytes);
    state.activate_document(a).unwrap();
    let stale = current_request(&state);
    wait_for_page(&mut state, &wake, PageId::new(1));
    let old_png = state.pdf_controller.state().png.clone();
    state.activate_document(b).unwrap();
    let current = current_request(&state);
    assert_ne!(stale.document_id(), current.document_id());
    assert!(current.request_id() > stale.request_id());
    state.close_document(a).unwrap();
    assert_eq!(state.active_document, Some(b));
    assert_eq!(current_request(&state), current);
    assert!(!state.adopt_pdf_raster_result(stale, Ok(old_png)));
    assert!(!state.adopt_pdf_raster_result(stale, Err(pdf_core::PdfError::new("closed"))));
    wait_for_page(&mut state, &wake, PageId::new(1));
    state.close_document(b).unwrap();
    assert!(state.pdf_raster_batch.is_none());
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Empty);
}

#[test]
fn returning_to_same_session_rejects_earlier_generation() {
    let bytes = include_bytes!("../../../../fixtures/public/pdf/cid-unicode-space-control.pdf");
    let (mut state, wake) = state_with_wake();
    let a = register_pdf(&mut state, bytes);
    let b = register_pdf(&mut state, bytes);
    state.activate_document(a).unwrap();
    let old_a = current_request(&state);
    state.activate_document(b).unwrap();
    state.activate_document(a).unwrap();
    let new_a = current_request(&state);
    assert_eq!(old_a.document_id(), new_a.document_id());
    assert_eq!(old_a.page_id(), new_a.page_id());
    assert_eq!(old_a.scale().to_bits(), new_a.scale().to_bits());
    assert!(new_a.request_id() > old_a.request_id());
    assert!(!state.adopt_pdf_raster_result(old_a, Ok(vec![1, 2, 3])));
    assert!(!state.adopt_pdf_raster_result(old_a, Err(pdf_core::PdfError::new("late"))));
    assert_eq!(current_request(&state), new_a);
    wait_for_page(&mut state, &wake, PageId::new(1));
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Ready);
}

#[test]
fn closing_active_session_rejects_inflight_completion() {
    let bytes = include_bytes!("../../../../fixtures/public/pdf/cid-unicode-space-control.pdf");
    let (mut state, _wake) = state_with_wake();
    let id = register_pdf(&mut state, bytes);
    state.activate_document(id).unwrap();
    let pending = current_request(&state);
    state.close_document(id).unwrap();
    assert!(!state.adopt_pdf_raster_result(pending, Ok(vec![1, 2, 3])));
    assert!(!state.adopt_pdf_raster_result(pending, Err(pdf_core::PdfError::new("late"))));
    assert!(state.pdf_raster_batch.is_none());
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Empty);
}

#[test]
fn neighbor_error_preserves_current_page_without_repeated_submission() {
    let (mut state, wake) = state_with_wake();
    let id = register_pdf(&mut state, &tiny_two_page_pdf());
    state.activate_document(id).unwrap();
    wait_for_page(&mut state, &wake, PageId::new(1));
    let current_png = state.pdf_controller.state().png.clone();
    let neighbor = current_request(&state);
    assert_eq!(neighbor.page_id(), PageId::new(2));
    assert!(state.adopt_pdf_raster_result(neighbor, Err(pdf_core::PdfError::new("neighbor"))));
    let sequence = state.pdf_request_sequence;
    state.pdf_effect = state.pdf_controller.request_current_page();
    state.apply_pdf_effect();
    assert_eq!(state.pdf_request_sequence, sequence);
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Ready);
    assert_eq!(state.pdf_controller.state().png, current_png);
}

#[test]
fn explicit_retry_requeues_error_but_refresh_does_not_spin() {
    let bytes = include_bytes!("../../../../fixtures/public/pdf/cid-unicode-space-control.pdf");
    let (mut state, wake) = state_with_wake();
    let id = register_pdf(&mut state, bytes);
    state.activate_document(id).unwrap();
    let stale = current_request(&state);
    assert!(state.adopt_pdf_raster_result(stale, Err(pdf_core::PdfError::new("controlled"))));
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Error);
    let sequence = state.pdf_request_sequence;
    state.pdf_effect = state.pdf_controller.request_current_page();
    state.apply_pdf_effect();
    assert_eq!(state.pdf_request_sequence, sequence);
    state.pdf_go_to_page(1);
    assert!(current_request(&state).request_id() > stale.request_id());
    assert!(!state.adopt_pdf_raster_result(stale, Err(pdf_core::PdfError::new("late"))));
    wait_for_page(&mut state, &wake, PageId::new(1));
    assert_eq!(state.pdf_controller.state().status, PdfViewStatus::Ready);
}

#[test]
fn large_resident_page_survives_cache_eviction_without_resubmission() {
    let mut state = AppState::default();
    let backend = DocumentId::new(101);
    let page = PageId::new(1);
    state
        .pdf_controller
        .state_mut()
        .bind_pdf(backend, 1, &[page]);
    let bytes = vec![1; PDF_VIEW_CACHE_MAX_BYTES + 1];
    state
        .pdf_controller
        .cache
        .insert(backend, page, 1.0, bytes.clone());
    assert!(state.pdf_controller.cache.is_empty());
    state
        .pdf_controller
        .state_mut()
        .visible_pages
        .push(RenderedPage {
            page_index: page,
            scale: 1.0,
            png: bytes.clone(),
        });
    state.refresh_pdf_cached_pages();
    assert_eq!(state.pdf_controller.state().png, bytes);
    assert_eq!(state.pdf_controller.state().visible_pages.len(), 1);
}
