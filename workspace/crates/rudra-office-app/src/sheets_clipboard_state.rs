//! Application clipboard ownership. This is not a second mutation/history stack.
//! Rich Copy is immutable; Cut stays source-backed until one canonical paste.
use crate::RuntimeDocumentId;
use sheets_core::{CellRange, Revision};

pub(super) const MAX_CLIPBOARD_TEXT_BYTES: usize = 1024 * 1024;

/// A native intercepted intent precedes every later application event, not only
/// the eventual timer Signal. Taking the bounded slot makes delivery once-only.
pub(super) fn flush_clipboard_intent_before_event<T>(pending: Option<T>, apply: impl FnOnce(T)) {
    if let Some(intent) = pending {
        apply(intent);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SheetsClipboardCommand {
    Copy,
    Cut,
    Paste,
}

pub(super) struct ClipboardClaim<P> {
    pub source: RuntimeDocumentId,
    pub revision: Revision,
    pub range: CellRange,
    pub pending_cut: bool,
    pub payload: P,
    pub text: String,
    pub sequence: u32,
}

pub(super) struct SheetsClipboardState<P> {
    claim: Option<ClipboardClaim<P>>,
}

impl<P> Default for SheetsClipboardState<P> {
    fn default() -> Self {
        Self { claim: None }
    }
}

impl<P> SheetsClipboardState<P> {
    /// Replace only after capture AND OS write verification succeeded.
    pub fn replace(&mut self, claim: ClipboardClaim<P>) {
        self.claim = Some(claim);
    }

    pub fn matching(&self, sequence: u32, text: &str) -> Option<&ClipboardClaim<P>> {
        self.claim
            .as_ref()
            .filter(|claim| claim.sequence == sequence && claim.text == text)
    }

    pub fn pending(&self) -> Option<&ClipboardClaim<P>> {
        self.claim.as_ref().filter(|claim| claim.pending_cut)
    }

    pub fn cancel_cut(&mut self) -> bool {
        if self.pending().is_some() {
            self.claim = None;
            true
        } else {
            false
        }
    }

    pub fn source_changed(&mut self, source: RuntimeDocumentId, revision: Revision) {
        if self
            .pending()
            .is_some_and(|claim| claim.source == source && claim.revision != revision)
        {
            self.cancel_cut();
        }
    }

    pub fn source_closed(&mut self, source: RuntimeDocumentId) {
        if self.pending().is_some_and(|claim| claim.source == source) {
            self.cancel_cut();
        }
    }

    pub fn observe_sequence(&mut self, sequence: Option<u32>) {
        if sequence.is_some_and(|sequence| {
            self.pending()
                .is_some_and(|claim| claim.sequence != sequence)
        }) {
            self.cancel_cut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_runtime::DocumentRegistry;
    use sheets_core::{CellAddress, ColumnIndex, RowIndex, SheetId};

    #[test]
    fn clipboard_queued_paste_precedes_undo_before_signal_and_is_once_only() {
        let mut api =
            sheets_api::SpreadsheetApi::new(sheets_ironcalc_adapter::IroncalcAdapter::new());
        let address = CellAddress::new(SheetId(1), RowIndex(0), ColumnIndex(0));
        let range = CellRange::new(address, address).unwrap();
        let mut pending = Some(range);
        flush_clipboard_intent_before_event(pending.take(), |range| {
            api.paste_tsv("42", range).unwrap();
        });
        api.undo().unwrap(); // queued Ctrl+Z, before Signal
        assert_eq!(api.cell_view(SheetId(1), address).unwrap().raw_text(), "");
        flush_clipboard_intent_before_event(pending.take(), |_| panic!("duplicate delivery"));
        api.redo().unwrap();
        assert_eq!(api.cell_view(SheetId(1), address).unwrap().raw_text(), "42");
    }

    fn claim(cut: bool) -> ClipboardClaim<&'static str> {
        // Typed IDs come from the existing allocator, never synthetic paths/indexes.
        let mut registry = DocumentRegistry::default();
        let api = sheets_api::SpreadsheetApi::new(sheets_ironcalc_adapter::IroncalcAdapter::new());
        let session = crate::document_runtime::DocumentSession::new(
            crate::document_runtime::DocumentSource::from_path("clipboard-test.xlsx".into()),
            crate::OpenDocument::Sheets(api.document().snapshot()),
        )
        .unwrap();
        let source = registry.insert(session).unwrap();
        let address = CellAddress::new(SheetId(1), RowIndex(0), ColumnIndex(0));
        ClipboardClaim {
            source,
            revision: Revision(1),
            range: CellRange::new(address, address).unwrap(),
            pending_cut: cut,
            payload: "snapshot",
            text: "Русский 😀".into(),
            sequence: 31,
        }
    }

    #[test]
    fn identical_external_text_with_new_sequence_never_matches_rich() {
        let mut state = SheetsClipboardState::default();
        state.replace(claim(false));
        assert!(state.matching(31, "Русский 😀").is_some());
        assert!(state.matching(32, "Русский 😀").is_none());
        assert!(state.matching(31, "external").is_none());
    }

    #[test]
    fn source_changes_cancel_cut_but_do_not_destroy_immutable_copy() {
        let mut state = SheetsClipboardState::default();
        let copy = claim(false);
        let source = copy.source;
        state.replace(copy);
        state.source_changed(source, Revision(2));
        state.source_closed(source);
        assert!(state.matching(31, "Русский 😀").is_some());
        let cut = claim(true);
        let cut_source = cut.source;
        state.replace(cut);
        state.source_changed(cut_source, Revision(2));
        assert!(state.pending().is_none());
        assert!(state.matching(31, "Русский 😀").is_none());
    }

    #[test]
    fn escape_cancels_cut_only() {
        let mut state = SheetsClipboardState::default();
        state.replace(claim(false));
        assert!(!state.cancel_cut());
        assert!(state.matching(31, "Русский 😀").is_some());
        state.replace(claim(true));
        assert!(state.cancel_cut());
        assert!(!state.cancel_cut());
    }

    #[test]
    fn external_overwrite_cancels_pending_cut_but_read_unavailability_does_not() {
        let mut state = SheetsClipboardState::default();
        state.replace(claim(true));
        state.observe_sequence(None);
        assert!(state.pending().is_some());
        state.observe_sequence(Some(32));
        assert!(state.pending().is_none());
    }
}
