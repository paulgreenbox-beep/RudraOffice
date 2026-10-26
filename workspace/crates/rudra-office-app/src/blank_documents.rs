//! Factory for new, untitled Office documents.
//!
//! The returned path is a temporary source for the normal document-open flow.
//! The caller owns its lifetime and must keep it until the session no longer
//! needs the source. Saving the edited document is a separate operation.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sheets_api::SpreadsheetApi;
use sheets_ironcalc_adapter::IroncalcAdapter;
use slides_api::{Presentation, PresentationId, Slide, SlideId};
use slides_pptx_adapter::office_oxide_adapter;
use writer_api::core::DocumentFormat;
use writer_api::{ExportedDocument, WriterApi};

static NEXT_TEMPORARY_DOCUMENT: AtomicU64 = AtomicU64::new(0);

pub use rudra_office_launcher::DocumentKind as BlankDocumentFormat;

/// Creates a new valid document in the process temporary directory.
///
/// The file is created exclusively, so an existing temporary document is
/// never overwritten. The caller should remove it after the document session
/// has been closed or saved to a permanent destination.
pub fn create_temporary(format: BlankDocumentFormat) -> Result<PathBuf, String> {
    let bytes = create_bytes(format)?;
    let directory = env::temp_dir().join("rudra-office").join("blank-documents");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Cannot create temporary document directory: {error}"))?;

    for _ in 0..128 {
        let sequence = NEXT_TEMPORARY_DOCUMENT.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("Cannot timestamp temporary document: {error}"))?
            .as_nanos();
        let path = directory.join(format!(
            "untitled-{}-{}-{timestamp}-{sequence}.{}",
            std::process::id(),
            format.extension(),
            format.extension(),
        ));

        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
                    drop(file);
                    let _ = fs::remove_file(&path);
                    return Err(format!("Cannot write temporary document: {error}"));
                }
                return Ok(path);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create temporary document: {error}")),
        }
    }

    Err("Cannot allocate a unique temporary document name".to_owned())
}

fn create_bytes(format: BlankDocumentFormat) -> Result<Vec<u8>, String> {
    match format {
        BlankDocumentFormat::Docx => WriterApi::new()
            .export_docx()
            .map_err(|error| format!("Cannot create DOCX: {error}")),
        BlankDocumentFormat::Xlsx => SpreadsheetApi::<IroncalcAdapter>::production()
            .export_xlsx()
            .map_err(|error| format!("Cannot create XLSX: {error}")),
        BlankDocumentFormat::Pptx => {
            let mut presentation = Presentation::new(PresentationId::new(1));
            presentation
                .add_slide(Slide::new(SlideId::new(1)))
                .map_err(|error| format!("Cannot create blank slide: {error}"))?;
            office_oxide_adapter()
                .save_bytes(&presentation)
                .map_err(|error| format!("Cannot create PPTX: {error}"))
        }
        BlankDocumentFormat::Pdf => match WriterApi::new()
            .export(DocumentFormat::Pdf)
            .map_err(|error| format!("Cannot create PDF: {error}"))?
        {
            ExportedDocument::Pdf(bytes) => Ok(bytes),
            _ => Err("Writer returned an unexpected PDF export format".to_owned()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_oxide_adapter::PdfOxideAdapter;

    struct TemporaryDocument(PathBuf);

    impl TemporaryDocument {
        fn new(format: BlankDocumentFormat) -> Self {
            let path = create_temporary(format).expect("temporary document must be created");
            assert!(path.starts_with(env::temp_dir()));
            assert_eq!(
                path.extension().and_then(|value| value.to_str()),
                Some(format.extension())
            );
            Self(path)
        }

        fn bytes(&self) -> Vec<u8> {
            fs::read(&self.0).expect("temporary document must be readable")
        }
    }

    impl Drop for TemporaryDocument {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn blank_docx_reopens_in_writer() {
        let document = TemporaryDocument::new(BlankDocumentFormat::Docx);
        WriterApi::new()
            .load_docx(&document.bytes())
            .expect("blank DOCX must reopen in Writer");
    }

    #[test]
    fn blank_xlsx_reopens_in_sheets() {
        let document = TemporaryDocument::new(BlankDocumentFormat::Xlsx);
        SpreadsheetApi::<IroncalcAdapter>::production()
            .load_xlsx(&document.bytes())
            .expect("blank XLSX must reopen in Sheets");
    }

    #[test]
    fn blank_pptx_reopens_in_slides() {
        let document = TemporaryDocument::new(BlankDocumentFormat::Pptx);
        let presentation = office_oxide_adapter()
            .load_bytes(&document.bytes())
            .expect("blank PPTX must reopen in Slides");
        assert_eq!(presentation.slides.len(), 1);
    }

    #[test]
    fn blank_pdf_reopens_in_pdf_viewer() {
        let document = TemporaryDocument::new(BlankDocumentFormat::Pdf);
        let artifact = PdfOxideAdapter::default()
            .parse_detached(&document.bytes())
            .expect("blank PDF must reopen in PDF viewer");
        assert_eq!(artifact.document_info().page_count, 1);
    }

    #[test]
    fn temporary_documents_have_unique_paths() {
        let first = TemporaryDocument::new(BlankDocumentFormat::Docx);
        let second = TemporaryDocument::new(BlankDocumentFormat::Docx);
        assert_ne!(first.0, second.0);
        assert!(first.0.exists());
        assert!(second.0.exists());
    }
}
