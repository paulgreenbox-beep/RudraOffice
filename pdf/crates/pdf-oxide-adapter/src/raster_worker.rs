//! One CPU raster thread with one replaceable pending job and one completion.
//!
//! Cancellation invalidates work and discards late results. It does not interrupt
//! a vendor render already in progress, which can continue to occupy this worker.
//! The application must also validate its typed session before GPU adoption.

use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex, MutexGuard,
};
use std::thread::{self, JoinHandle};

use pdf_core::{DocumentId, PageId, PdfError, PdfResult};
use pdf_oxide::document::PdfDocument;

use crate::{error, render_document_page, validate_render_scale, MAX_RENDER_PNG_BYTES};

const MAX_WORKER_ERROR_MESSAGE_BYTES: usize = 1024;
const WORKER_ERROR_TRUNCATION_MARKER: &str = "...[truncated]";

/// Exact raster identity. Request IDs must increase across a worker's lifetime,
/// including document switches, cancellations and requests at the same scale.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PdfRenderRequest {
    document_id: DocumentId,
    page_id: PageId,
    scale_bits: u32,
    request_id: u64,
}

impl PdfRenderRequest {
    pub fn new(
        document_id: DocumentId,
        page_id: PageId,
        scale: f32,
        request_id: u64,
    ) -> PdfResult<Self> {
        if document_id.value() == 0 || page_id.value() == 0 || request_id == 0 {
            return Err(error(
                "PDF render document, page and request IDs must be nonzero",
            ));
        }
        validate_render_scale(scale)?;
        Ok(Self {
            document_id,
            page_id,
            scale_bits: scale.to_bits(),
            request_id,
        })
    }

    pub const fn document_id(self) -> DocumentId {
        self.document_id
    }

    pub const fn page_id(self) -> PageId {
        self.page_id
    }

    pub fn scale(self) -> f32 {
        f32::from_bits(self.scale_bits)
    }

    pub const fn scale_bits(self) -> u32 {
        self.scale_bits
    }

    pub const fn request_id(self) -> u64 {
        self.request_id
    }
}

/// Owned, transferable CPU work sharing the already parsed document.
/// No adapter registry borrow, UI handle, GPU resource or source-byte copy.
pub struct PdfRenderJob {
    request: PdfRenderRequest,
    document: Arc<PdfDocument>,
    page_index: usize,
    live: Arc<AtomicBool>,
}

impl PdfRenderJob {
    pub(crate) fn new(
        request: PdfRenderRequest,
        document: Arc<PdfDocument>,
        page_index: usize,
        live: Arc<AtomicBool>,
    ) -> Self {
        Self {
            request,
            document,
            page_index,
            live,
        }
    }

    pub const fn request(&self) -> PdfRenderRequest {
        self.request
    }

    /// Execute synchronously on the calling thread, preferably the CPU worker.
    /// All existing production content/raster/PNG budgets remain in force.
    pub fn render(self) -> PdfResult<Vec<u8>> {
        if !self.is_live() {
            return Err(error("PDF render document is closed"));
        }
        let result = render_document_page(
            &self.document,
            self.page_index,
            self.request.page_id(),
            self.request.scale(),
        );
        if !self.is_live() {
            return Err(error("PDF render document closed before completion"));
        }
        result
    }

    fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
}

impl fmt::Debug for PdfRenderJob {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PdfRenderJob")
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}

/// CPU result, retaining the exact request identity even for controlled errors.
/// Taking a completion does not authorize application-session or GPU adoption.
#[derive(Debug)]
pub struct PdfRenderCompletion {
    pub request: PdfRenderRequest,
    pub result: PdfResult<Vec<u8>>,
    live: Arc<AtomicBool>,
}

#[derive(Default)]
struct State {
    last_request_id: u64,
    latest: Option<PdfRenderRequest>,
    pending: Option<PdfRenderJob>,
    in_flight: Option<PdfRenderRequest>,
    completion: Option<PdfRenderCompletion>,
    shutdown: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

/// Create exactly one instance for the application's PDF raster queue.
///
/// Queue/result slots are bounded; this is not a total RSS or CPU-time cap.
/// The notifier runs on the worker after publication and outside the queue lock.
/// Use it only to wake the existing application event loop; do not draw there.
pub struct PdfRenderWorker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl PdfRenderWorker {
    pub fn new(notify: impl Fn() + Send + 'static) -> PdfResult<Self> {
        Self::with_renderer(notify, PdfRenderJob::render)
    }

    /// Replace pending work and discard any unread older completion.
    /// An old/reused ID, a closed document or a stopped worker is rejected
    /// without disturbing a newer request.
    pub fn submit_latest(&self, job: PdfRenderJob) -> PdfResult<()> {
        if !job.is_live() {
            return Err(error("cannot queue a render for a closed PDF document"));
        }
        let mut state = lock_state(&self.shared);
        if state.shutdown {
            drop(state);
            drop(job);
            return Err(error("PDF raster worker is shut down"));
        }
        let request = job.request();
        if request.request_id() <= state.last_request_id {
            drop(state);
            drop(job);
            return Err(error("PDF render request IDs must be strictly increasing"));
        }
        state.last_request_id = request.request_id();
        state.latest = Some(request);
        let old_pending = state.pending.replace(job);
        let old_completion = state.completion.take();
        drop(state);
        drop(old_pending);
        drop(old_completion);
        self.shared.wake.notify_one();
        Ok(())
    }

    /// Cancel this exact request without cancelling a newer request.
    /// In-flight vendor work is not preempted; its result will be discarded.
    pub fn cancel(&self, request_id: u64) {
        let mut state = lock_state(&self.shared);
        if state
            .latest
            .is_some_and(|request| request.request_id() == request_id)
        {
            state.latest = None;
        }
        let pending = if state
            .pending
            .as_ref()
            .is_some_and(|job| job.request().request_id() == request_id)
        {
            state.pending.take()
        } else {
            None
        };
        let completion = if state
            .completion
            .as_ref()
            .is_some_and(|result| result.request.request_id() == request_id)
        {
            state.completion.take()
        } else {
            None
        };
        drop(state);
        drop(pending);
        drop(completion);
    }

    /// Invalidate queued/results for one backend document. The application
    /// must also call the adapter's close_document to invalidate prepared jobs.
    pub fn cancel_document(&self, document_id: DocumentId) {
        let mut state = lock_state(&self.shared);
        if state
            .latest
            .is_some_and(|request| request.document_id() == document_id)
        {
            state.latest = None;
        }
        let pending = if state
            .pending
            .as_ref()
            .is_some_and(|job| job.request().document_id() == document_id)
        {
            state.pending.take()
        } else {
            None
        };
        let completion = if state
            .completion
            .as_ref()
            .is_some_and(|result| result.request.document_id() == document_id)
        {
            state.completion.take()
        } else {
            None
        };
        drop(state);
        drop(pending);
        drop(completion);
    }

    pub fn take_completion(&self) -> Option<PdfRenderCompletion> {
        let mut state = lock_state(&self.shared);
        let completion = state.completion.take()?;
        let current = state.latest == Some(completion.request)
            && state.in_flight != Some(completion.request)
            && completion.live.load(Ordering::Acquire)
            && !state.shutdown;
        drop(state);
        current.then_some(completion)
    }

    /// Nonblocking shutdown. Discards queued/results and prevents late
    /// publication, but lets the current vendor call return in its own time.
    pub fn shutdown(&self) {
        let mut state = lock_state(&self.shared);
        state.shutdown = true;
        state.latest = None;
        let pending = state.pending.take();
        let completion = state.completion.take();
        drop(state);
        drop(pending);
        drop(completion);
        self.shared.wake.notify_one();
    }

    fn with_renderer<N, F>(notify: N, mut render: F) -> PdfResult<Self>
    where
        N: Fn() + Send + 'static,
        F: FnMut(PdfRenderJob) -> PdfResult<Vec<u8>> + Send + 'static,
    {
        let shared = Arc::new(Shared::default());
        let worker_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("rudra-pdf-raster".into())
            .spawn(move || loop {
                let job = {
                    let mut state = lock_state(&worker_shared);
                    while state.pending.is_none() && !state.shutdown {
                        state = worker_shared
                            .wake
                            .wait(state)
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                    }
                    if state.shutdown {
                        return;
                    }
                    let job = state.pending.take().expect("pending work was checked");
                    if state.latest != Some(job.request()) || !job.is_live() {
                        drop(state);
                        drop(job);
                        continue;
                    }
                    state.in_flight = Some(job.request());
                    job
                };
                let request = job.request();
                let live = Arc::clone(&job.live);
                // A backend panic becomes a controlled request failure rather
                // than leaving this fixed worker permanently unavailable.
                let result = catch_unwind(AssertUnwindSafe(|| render(job))).unwrap_or_else(|_| {
                    Err(error("PDF CPU raster panicked; page was not rendered"))
                });
                let result = match result {
                    Ok(png) if png.len() > MAX_RENDER_PNG_BYTES => {
                        Err(error("PDF worker completion exceeds the PNG byte budget"))
                    }
                    Err(render_error) => Err(bound_worker_error(render_error)),
                    result => result,
                };
                let (published, displaced_completion) = {
                    let mut state = lock_state(&worker_shared);
                    state.in_flight = None;
                    if !state.shutdown
                        && state.latest == Some(request)
                        && live.load(Ordering::Acquire)
                    {
                        let displaced_completion = state.completion.replace(PdfRenderCompletion {
                            request,
                            result,
                            live,
                        });
                        (true, displaced_completion)
                    } else {
                        (false, None)
                    }
                };
                drop(displaced_completion);
                if published {
                    // The completion remains available if the caller's wake
                    // callback fails; a notifier panic cannot kill the worker.
                    let _ = catch_unwind(AssertUnwindSafe(&notify));
                }
            })
            .map_err(|err| error(format!("cannot start PDF raster worker: {err}")))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }
}

impl Drop for PdfRenderWorker {
    fn drop(&mut self) {
        self.shutdown();
        // Dropping the handle detaches it. Never join a nonpreemptible raster
        // from the UI/drop path; the thread owns its state until it exits.
        drop(self.thread.take());
    }
}

fn lock_state(shared: &Shared) -> MutexGuard<'_, State> {
    shared
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn bound_worker_error(error: PdfError) -> PdfError {
    let message = error.message();
    if message.len() <= MAX_WORKER_ERROR_MESSAGE_BYTES {
        return error;
    }

    let prefix_limit = MAX_WORKER_ERROR_MESSAGE_BYTES - WORKER_ERROR_TRUNCATION_MARKER.len();
    let prefix_end = message
        .char_indices()
        .map(|(index, ch)| index + ch.len_utf8())
        .take_while(|end| *end <= prefix_limit)
        .last()
        .unwrap_or(0);
    let mut bounded = String::with_capacity(prefix_end + WORKER_ERROR_TRUNCATION_MARKER.len());
    bounded.push_str(&message[..prefix_end]);
    bounded.push_str(WORKER_ERROR_TRUNCATION_MARKER);
    PdfError::new(bounded)
}

const _: () = {
    fn _assert_send<T: Send>() {}
    fn _check() {
        _assert_send::<PdfRenderRequest>();
        _assert_send::<PdfRenderJob>();
        _assert_send::<PdfRenderCompletion>();
        _assert_send::<PdfRenderWorker>();
    }
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PdfOxideAdapter;
    use pdf_core::PdfEngine;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::time::Duration;

    const FAILURE_BOUND: Duration = Duration::from_secs(5);

    fn tiny_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 80 60] /Resources << >> /Contents 4 0 R >>",
            "<< /Length 0 >>\nstream\n\nendstream",
        ];
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes(),
        );
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

    fn engine() -> (PdfOxideAdapter, DocumentId) {
        let mut adapter = PdfOxideAdapter::default();
        let document = adapter.parse(&tiny_pdf()).unwrap().document_id;
        (adapter, document)
    }

    fn job(adapter: &PdfOxideAdapter, document: DocumentId, id: u64) -> PdfRenderJob {
        adapter
            .prepare_render(PdfRenderRequest::new(document, PageId::new(1), 1.0, id).unwrap())
            .unwrap()
    }

    struct Harness {
        worker: PdfRenderWorker,
        entered: Receiver<u64>,
        resume: Sender<()>,
        notified: Receiver<()>,
    }

    impl Harness {
        fn new(panic_on_first: bool) -> Self {
            let (entered_tx, entered) = mpsc::channel();
            let (resume, resume_rx) = mpsc::channel();
            let (notified_tx, notified) = mpsc::channel();
            let worker = PdfRenderWorker::with_renderer(
                move || {
                    let _ = notified_tx.send(());
                },
                move |job| {
                    let id = job.request().request_id();
                    entered_tx.send(id).unwrap();
                    resume_rx.recv().unwrap();
                    assert!(!(panic_on_first && id == 1), "injected raster failure");
                    // The fake proves queue lifecycle, not PNG correctness.
                    Ok(id.to_le_bytes().to_vec())
                },
            )
            .unwrap();
            Self {
                worker,
                entered,
                resume,
                notified,
            }
        }

        fn started(&self, id: u64) {
            assert_eq!(self.entered.recv_timeout(FAILURE_BOUND).unwrap(), id);
        }

        fn complete(&self, id: u64) {
            self.resume.send(()).unwrap();
            self.notified.recv_timeout(FAILURE_BOUND).unwrap();
            let completion = self.worker.take_completion().unwrap();
            assert_eq!(completion.request.request_id(), id);
            assert_eq!(completion.result.unwrap(), id.to_le_bytes());
        }
    }

    #[test]
    fn jobs_share_the_parsed_document_without_a_source_clone_or_reparse() {
        let (adapter, document) = engine();
        let first = job(&adapter, document, 1);
        let second = job(&adapter, document, 2);
        let stored = &adapter.documents.get(&document).unwrap().document;
        assert!(Arc::ptr_eq(&first.document, stored));
        assert!(Arc::ptr_eq(&second.document, stored));
        assert_eq!(Arc::strong_count(stored), 3);
    }

    #[test]
    fn a_slow_job_keeps_only_one_latest_pending_and_discards_its_late_result() {
        let (adapter, document) = engine();
        let harness = Harness::new(false);
        harness
            .worker
            .submit_latest(job(&adapter, document, 1))
            .unwrap();
        harness.started(1);
        for id in 2..=100 {
            harness
                .worker
                .submit_latest(job(&adapter, document, id))
                .unwrap();
        }
        {
            let state = lock_state(&harness.worker.shared);
            assert_eq!(state.in_flight.unwrap().request_id(), 1);
            assert_eq!(state.pending.as_ref().unwrap().request().request_id(), 100);
            assert!(state.completion.is_none());
        }
        assert!(harness
            .worker
            .submit_latest(job(&adapter, document, 50))
            .is_err());
        harness.resume.send(()).unwrap();
        harness.started(100);
        assert!(harness.worker.take_completion().is_none());
        harness.complete(100);
    }

    #[test]
    fn cancelled_pending_and_running_work_cannot_replace_a_newer_result() {
        let (adapter, document) = engine();
        let harness = Harness::new(false);
        harness
            .worker
            .submit_latest(job(&adapter, document, 1))
            .unwrap();
        harness.started(1);
        harness.worker.cancel(1);
        harness
            .worker
            .submit_latest(job(&adapter, document, 2))
            .unwrap();
        harness.worker.cancel(2);
        assert!(lock_state(&harness.worker.shared).pending.is_none());
        harness
            .worker
            .submit_latest(job(&adapter, document, 3))
            .unwrap();
        harness.worker.cancel(1);
        harness.resume.send(()).unwrap();
        harness.started(3);
        assert!(harness.worker.take_completion().is_none());
        harness.complete(3);
    }

    #[test]
    fn close_during_running_work_cancels_pending_and_drops_late_pixels() {
        let (mut adapter, document) = engine();
        let other = adapter.parse(&tiny_pdf()).unwrap().document_id;
        let harness = Harness::new(false);
        harness
            .worker
            .submit_latest(job(&adapter, document, 1))
            .unwrap();
        harness.started(1);
        harness
            .worker
            .submit_latest(job(&adapter, document, 2))
            .unwrap();
        let prepared_before_close = job(&adapter, document, 4);
        harness.worker.cancel_document(document);
        adapter.close_document(document).unwrap();
        assert!(harness.worker.submit_latest(prepared_before_close).is_err());
        assert!(adapter
            .prepare_render(PdfRenderRequest::new(document, PageId::new(1), 1.0, 5).unwrap())
            .is_err());
        harness
            .worker
            .submit_latest(job(&adapter, other, 3))
            .unwrap();
        harness.resume.send(()).unwrap();
        harness.started(3);
        assert!(harness.worker.take_completion().is_none());
        harness.complete(3);
    }

    #[test]
    fn closing_after_publication_discards_the_unread_completion() {
        let (mut adapter, document) = engine();
        let harness = Harness::new(false);
        harness
            .worker
            .submit_latest(job(&adapter, document, 1))
            .unwrap();
        harness.started(1);
        harness.resume.send(()).unwrap();
        harness.notified.recv_timeout(FAILURE_BOUND).unwrap();
        adapter.close_document(document).unwrap();
        assert!(harness.worker.take_completion().is_none());
    }

    #[test]
    fn newer_submission_discards_an_unread_completed_result() {
        let (adapter, document) = engine();
        let harness = Harness::new(false);
        harness
            .worker
            .submit_latest(job(&adapter, document, 1))
            .unwrap();
        harness.started(1);
        harness.resume.send(()).unwrap();
        harness.notified.recv_timeout(FAILURE_BOUND).unwrap();
        harness
            .worker
            .submit_latest(job(&adapter, document, 2))
            .unwrap();
        harness.started(2);
        assert!(harness.worker.take_completion().is_none());
        harness.complete(2);
    }

    #[test]
    fn shutdown_and_drop_finish_before_an_in_flight_renderer_is_released() {
        let (adapter, document) = engine();
        let Harness {
            worker,
            entered,
            resume,
            notified,
        } = Harness::new(false);
        worker.submit_latest(job(&adapter, document, 1)).unwrap();
        assert_eq!(entered.recv_timeout(FAILURE_BOUND).unwrap(), 1);
        worker.shutdown();
        assert!(worker.submit_latest(job(&adapter, document, 2)).is_err());
        let (dropped_tx, dropped_rx) = mpsc::channel();
        let dropping = thread::spawn(move || {
            drop(worker);
            dropped_tx.send(()).unwrap();
        });
        // If Drop joined the renderer, this cannot arrive before resume.
        dropped_rx.recv_timeout(FAILURE_BOUND).unwrap();
        dropping.join().unwrap();
        resume.send(()).unwrap();
        assert!(matches!(
            notified.recv_timeout(FAILURE_BOUND),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn backend_panic_is_a_request_error_and_the_same_worker_can_continue() {
        let (adapter, document) = engine();
        let harness = Harness::new(true);
        harness
            .worker
            .submit_latest(job(&adapter, document, 1))
            .unwrap();
        harness.started(1);
        harness.resume.send(()).unwrap();
        harness.notified.recv_timeout(FAILURE_BOUND).unwrap();
        let completion = harness.worker.take_completion().unwrap();
        assert!(completion
            .result
            .unwrap_err()
            .message()
            .contains("panicked"));
        harness
            .worker
            .submit_latest(job(&adapter, document, 2))
            .unwrap();
        harness.started(2);
        harness.complete(2);
    }

    #[test]
    fn oversized_unicode_worker_error_is_bounded_on_a_utf8_boundary() {
        let (adapter, document) = engine();
        let (notified_tx, notified) = mpsc::channel();
        let oversized_message = "界".repeat(MAX_WORKER_ERROR_MESSAGE_BYTES * 4);
        let worker = PdfRenderWorker::with_renderer(
            move || {
                let _ = notified_tx.send(());
            },
            move |_| Err(PdfError::new(oversized_message.clone())),
        )
        .unwrap();

        worker.submit_latest(job(&adapter, document, 1)).unwrap();
        notified.recv_timeout(FAILURE_BOUND).unwrap();

        let completion = worker.take_completion().unwrap();
        let message = completion.result.unwrap_err().message().to_owned();
        let expected_prefix_chars =
            (MAX_WORKER_ERROR_MESSAGE_BYTES - WORKER_ERROR_TRUNCATION_MARKER.len()) / "界".len();
        let expected = format!(
            "{}{}",
            "界".repeat(expected_prefix_chars),
            WORKER_ERROR_TRUNCATION_MARKER
        );
        assert_eq!(message, expected);
        assert!(message.len() <= MAX_WORKER_ERROR_MESSAGE_BYTES);
        worker.shutdown();
    }

    #[test]
    fn oversized_png_is_replaced_with_a_bounded_error_and_worker_continues() {
        let (adapter, document) = engine();
        let (entered_tx, entered) = mpsc::channel();
        let (resume, resume_rx) = mpsc::channel();
        let (notified_tx, notified) = mpsc::channel();
        let worker = PdfRenderWorker::with_renderer(
            move || {
                let _ = notified_tx.send(());
            },
            move |job| {
                let id = job.request().request_id();
                entered_tx.send(id).unwrap();
                resume_rx.recv().unwrap();
                if id == 1 {
                    Ok(vec![0; MAX_RENDER_PNG_BYTES + 1])
                } else {
                    Ok(id.to_le_bytes().to_vec())
                }
            },
        )
        .unwrap();

        worker.submit_latest(job(&adapter, document, 1)).unwrap();
        assert_eq!(entered.recv_timeout(FAILURE_BOUND).unwrap(), 1);
        resume.send(()).unwrap();
        notified.recv_timeout(FAILURE_BOUND).unwrap();
        let oversized = worker.take_completion().unwrap();
        assert_eq!(oversized.request.request_id(), 1);
        assert_eq!(
            oversized.result.unwrap_err().message(),
            "PDF worker completion exceeds the PNG byte budget"
        );

        worker.submit_latest(job(&adapter, document, 2)).unwrap();
        assert_eq!(entered.recv_timeout(FAILURE_BOUND).unwrap(), 2);
        resume.send(()).unwrap();
        notified.recv_timeout(FAILURE_BOUND).unwrap();
        let later = worker.take_completion().unwrap();
        assert_eq!(later.request.request_id(), 2);
        assert_eq!(later.result.unwrap(), 2_u64.to_le_bytes());
        worker.shutdown();
    }

    #[test]
    fn stale_error_is_discarded_when_a_newer_request_is_pending() {
        let (adapter, document) = engine();
        let (entered_tx, entered) = mpsc::channel();
        let (resume, resume_rx) = mpsc::channel();
        let (notified_tx, notified) = mpsc::channel();
        let worker = PdfRenderWorker::with_renderer(
            move || {
                let _ = notified_tx.send(());
            },
            move |job| {
                let id = job.request().request_id();
                entered_tx.send(id).unwrap();
                resume_rx.recv().unwrap();
                if id == 1 {
                    Err(PdfError::new("stale raster failure"))
                } else {
                    Ok(id.to_le_bytes().to_vec())
                }
            },
        )
        .unwrap();

        worker.submit_latest(job(&adapter, document, 1)).unwrap();
        assert_eq!(entered.recv_timeout(FAILURE_BOUND).unwrap(), 1);
        worker.submit_latest(job(&adapter, document, 2)).unwrap();
        assert_eq!(
            lock_state(&worker.shared)
                .pending
                .as_ref()
                .unwrap()
                .request()
                .request_id(),
            2
        );

        resume.send(()).unwrap();
        assert_eq!(entered.recv_timeout(FAILURE_BOUND).unwrap(), 2);
        assert!(lock_state(&worker.shared).completion.is_none());
        assert!(matches!(
            notified.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        resume.send(()).unwrap();
        notified.recv_timeout(FAILURE_BOUND).unwrap();
        let latest = worker.take_completion().unwrap();
        assert_eq!(latest.request.request_id(), 2);
        assert_eq!(latest.result.unwrap(), 2_u64.to_le_bytes());
        worker.shutdown();
    }

    #[test]
    fn notifier_panic_keeps_completion_and_worker_available() {
        let (adapter, document) = engine();
        let (entered_tx, entered) = mpsc::channel();
        let (resume, resume_rx) = mpsc::channel();
        let (notified_tx, notified) = mpsc::channel();
        let notification_count = AtomicUsize::new(0);
        let worker = PdfRenderWorker::with_renderer(
            move || {
                let call = notification_count.fetch_add(1, Ordering::SeqCst);
                let _ = notified_tx.send(call + 1);
                if call == 0 {
                    panic!("injected notifier failure");
                }
            },
            move |job| {
                let id = job.request().request_id();
                entered_tx.send(id).unwrap();
                resume_rx.recv().unwrap();
                Ok(id.to_le_bytes().to_vec())
            },
        )
        .unwrap();

        worker.submit_latest(job(&adapter, document, 1)).unwrap();
        assert_eq!(entered.recv_timeout(FAILURE_BOUND).unwrap(), 1);
        resume.send(()).unwrap();
        assert_eq!(notified.recv_timeout(FAILURE_BOUND).unwrap(), 1);
        let first = worker.take_completion().unwrap();
        assert_eq!(first.request.request_id(), 1);
        assert_eq!(first.result.unwrap(), 1_u64.to_le_bytes());

        worker.submit_latest(job(&adapter, document, 2)).unwrap();
        assert_eq!(entered.recv_timeout(FAILURE_BOUND).unwrap(), 2);
        resume.send(()).unwrap();
        assert_eq!(notified.recv_timeout(FAILURE_BOUND).unwrap(), 2);
        let second = worker.take_completion().unwrap();
        assert_eq!(second.request.request_id(), 2);
        assert_eq!(second.result.unwrap(), 2_u64.to_le_bytes());
        worker.shutdown();
    }
}
