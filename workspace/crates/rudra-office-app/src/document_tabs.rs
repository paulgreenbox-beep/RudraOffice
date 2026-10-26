//! Multi-document tab projection and Office-specific tab strip.
//!
//! The tab strip is only a view of `AppState::documents`. It never allocates
//! identities and never stores a second document registry. Runtime IDs remain
//! the only target for activation and close actions.

use std::collections::HashMap;
use std::path::Path;

use makepad_widgets::widget::WidgetActionData;
use makepad_widgets::*;

use super::document_runtime::{DocumentId, DocumentSession};

const TAB_CLOSE_WIDTH: f64 = 24.0;
const TAB_DISPLAY_MAX_CHARS: usize = 30;

/// Data needed by the visual tab widget. It is intentionally not a session.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DocumentTabFrame {
    pub(crate) id: DocumentId,
    pub(crate) label: String,
    pub(crate) display_label: String,
    pub(crate) active: bool,
}

/// User actions emitted by the tab strip. Every action carries the public
/// runtime document identity rather than a tab index or path.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum DocumentTabsAction {
    #[default]
    None,
    Activate(DocumentId),
    Close(DocumentId),
}

/// Returns the deterministic next active ID after closing `closed`.
///
/// If the closed tab was active, the tab on its right wins; if it was the
/// last tab, the tab on its left wins. Closing an inactive tab leaves the
/// current active ID unchanged. A missing active ID is represented as `None`.
pub(crate) fn active_after_close<T: Copy + Eq>(
    ordered_ids: &[T],
    active: Option<T>,
    closed: T,
) -> Option<T> {
    let Some(position) = ordered_ids.iter().position(|id| *id == closed) else {
        return active.filter(|id| ordered_ids.contains(id));
    };
    if active != Some(closed) {
        return active.filter(|id| ordered_ids.contains(id));
    }
    ordered_ids.get(position + 1).copied().or_else(|| {
        position
            .checked_sub(1)
            .and_then(|index| ordered_ids.get(index).copied())
    })
}

/// Returns the next/previous live document for Ctrl+Tab navigation.
pub(crate) fn cycle_id<T: Copy + Eq>(
    ordered_ids: &[T],
    active: Option<T>,
    reverse: bool,
) -> Option<T> {
    if ordered_ids.is_empty() {
        return None;
    }
    let current = active
        .and_then(|id| ordered_ids.iter().position(|candidate| *candidate == id))
        .unwrap_or(if reverse {
            0
        } else {
            ordered_ids.len().saturating_sub(1)
        });
    let next = if reverse {
        if current == 0 {
            ordered_ids.len() - 1
        } else {
            current - 1
        }
    } else {
        (current + 1) % ordered_ids.len()
    };
    ordered_ids.get(next).copied()
}

/// Projects the registry's stable ID order into labels and active state.
/// Resolves a pointer inside the rendered tab rectangles.
///
/// The document ID comes from the runtime projection; geometry is only used to
/// distinguish the tab body from its close affordance.
#[cfg(test)]
fn tab_hit<T, I>(tabs: I, point: DVec2, close_width: f64) -> Option<(T, bool)>
where
    I: IntoIterator<Item = (T, Rect)>,
    T: Copy,
{
    tabs.into_iter().find_map(|(id, rect)| {
        if !rect.contains(point) {
            return None;
        }
        let close_start = rect.pos.x + (rect.size.x - close_width).max(0.0);
        Some((id, point.x >= close_start))
    })
}

#[cfg(test)]
fn action_for_hit(id: DocumentId, close: bool) -> DocumentTabsAction {
    if close {
        DocumentTabsAction::Close(id)
    } else {
        DocumentTabsAction::Activate(id)
    }
}

/// Produces a bounded visual caption while the full title remains available
/// to accessibility and runtime diagnostics. Middle ellipsis preserves both
/// the identifying prefix and the extension/duplicate ordinal at the end.
fn compact_tab_label(label: &str, max_chars: usize) -> String {
    let chars = label.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return label.to_owned();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars == 1 {
        return "…".to_owned();
    }
    let suffix_len = (max_chars / 3).clamp(1, 12).min(max_chars - 2);
    let prefix_len = max_chars - suffix_len - 1;
    let prefix = chars[..prefix_len].iter().collect::<String>();
    let suffix = chars[chars.len() - suffix_len..].iter().collect::<String>();
    format!("{prefix}…{suffix}")
}

/// Projects the registry's stable ID order into labels and active state.
pub(crate) fn project_document_tabs<'a, I>(
    sessions: I,
    active: Option<DocumentId>,
) -> Vec<DocumentTabFrame>
where
    I: IntoIterator<Item = (DocumentId, &'a DocumentSession)>,
{
    let entries = sessions
        .into_iter()
        .map(|(id, session)| (id, session.kind(), session.source().path()))
        .collect::<Vec<_>>();
    let mut counts = HashMap::<String, usize>::new();
    for (_, _, path) in &entries {
        *counts.entry(file_name(*path).to_owned()).or_default() += 1;
    }
    let mut seen = HashMap::<String, usize>::new();
    entries
        .into_iter()
        .map(|(id, _module, path)| {
            let name = file_name(path).to_owned();
            let label = if counts.get(&name).copied().unwrap_or(0) <= 1 {
                name
            } else {
                let ordinal = {
                    let entry = seen.entry(name.clone()).or_default();
                    *entry += 1;
                    *entry
                };
                format!("{name} · {ordinal}")
            };
            DocumentTabFrame {
                id,
                display_label: compact_tab_label(&label, TAB_DISPLAY_MAX_CHARS),
                label,
                active: active == Some(id),
            }
        })
        .collect()
}

fn file_name(path: Option<&Path>) -> &str {
    path.and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Untitled document")
}

/// Runtime-local mapping from an application DocumentId to a Makepad tab ID.
/// The Makepad ID is only a rendering/event key; it is never the document
/// identity or a second registry.
fn tab_live_id(id: DocumentId) -> LiveId {
    LiveId::from_str_num("rudra-office-document-tab", id.get())
}

/// Converts an upstream tab event back to the runtime-owned document ID.
///
/// `LiveId` is an implementation detail of Makepad rendering. It is only
/// looked up in the current projection and never escapes as document identity.
fn document_id_for_tab_live_id(frame: &[DocumentTabFrame], live_id: LiveId) -> Option<DocumentId> {
    frame
        .iter()
        .find(|tab| tab_live_id(tab.id) == live_id)
        .map(|tab| tab.id)
}

/// Returns the active index for this draw pass.
///
/// A resize uses one neutral draw to let upstream TabBar notice the active ID
/// transition again on the following draw, which starts its native reveal
/// scroll without changing the runtime document selection.
fn active_index_for_draw(frame: &[DocumentTabFrame], suppress_active_once: bool) -> Option<usize> {
    if suppress_active_once {
        None
    } else {
        frame.iter().position(|tab| tab.active)
    }
}

/// Makepad visual strip backed by the upstream TabBar implementation.
///
/// The widget owns only the current projection and forwards actions with the
/// original `DocumentId`. The upstream TabBar supplies the actual drawing and
/// close affordance; no Office state is stored in it.
#[derive(Script, ScriptHook, Widget)]
pub struct DocumentTabStrip {
    #[uid]
    uid: WidgetUid,
    #[source]
    source: ScriptObjectRef,
    #[walk]
    walk: Walk,
    #[layout]
    layout: Layout,

    #[redraw]
    #[live]
    tab_bar: TabBar,

    #[action_data]
    #[rust]
    action_data: WidgetActionData,
    #[rust]
    frame: Vec<DocumentTabFrame>,
    #[rust]
    reveal_active_after_resize: bool,
}

impl DocumentTabStrip {
    pub(crate) fn set_tabs(&mut self, cx: &mut Cx, frame: Vec<DocumentTabFrame>) {
        self.frame = frame;
        // The pinned TabBar's inherent redraw uses an unassigned view_area.
        // WidgetNode invalidates the actual ScrollBars owner.
        WidgetNode::redraw(&mut self.tab_bar, cx);
    }

    /// Requests the upstream TabBar to reveal the selected tab after its
    /// viewport changes. The application calls this after Window handles its
    /// geometry event, because that event does not propagate to child widgets.
    pub(crate) fn reveal_active_after_resize(&mut self, cx: &mut Cx) {
        if self.frame.iter().any(|tab| tab.active) {
            self.reveal_active_after_resize = true;
            self.tab_bar.set_active_tab_id(cx, None, Animate::No);
            WidgetNode::redraw(&mut self.tab_bar, cx);
        }
    }

    fn emit(&self, cx: &mut Cx, action: DocumentTabsAction) {
        cx.widget_action_with_data(&self.action_data, self.uid, action);
    }
}

impl Widget for DocumentTabStrip {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        // TabBar owns its horizontal scroll viewport and the automatic scroll
        // that brings the active tab into view. The wrapper must forward every
        // event, then translate only its semantic action back to DocumentId.
        for action in cx.capture_actions(|cx| self.tab_bar.handle_event(cx, event, scope)) {
            match action.as_widget_action().cast() {
                TabBarAction::TabWasPressed(live_id) => {
                    if let Some(id) = document_id_for_tab_live_id(&self.frame, live_id) {
                        self.emit(cx, DocumentTabsAction::Activate(id));
                    }
                }
                TabBarAction::TabCloseWasPressed(live_id) => {
                    if let Some(id) = document_id_for_tab_live_id(&self.frame, live_id) {
                        self.emit(cx, DocumentTabsAction::Close(id));
                    }
                }
                TabBarAction::ShouldTabStartDrag(_) | TabBarAction::None => {}
            }
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, _scope: &mut Scope, walk: Walk) -> DrawStep {
        let reset_active_once = std::mem::take(&mut self.reveal_active_after_resize);
        let active_index = active_index_for_draw(&self.frame, reset_active_once);
        self.tab_bar.begin(cx, active_index, walk);
        for tab in &self.frame {
            self.tab_bar.draw_tab(
                cx,
                tab_live_id(tab.id),
                &tab.display_label,
                id!(CloseableTab),
            );
        }
        self.tab_bar.end(cx);
        if reset_active_once {
            cx.redraw_all();
        }
        DrawStep::done()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenDocument;
    use crate::document_runtime::{DocumentSession, DocumentSource};
    use pdf_core::{DocumentId as PdfDocumentId, PageId};
    use rudra_runtime::Registry;
    use std::path::PathBuf;

    #[test]
    fn close_prefers_right_neighbor_then_left_neighbor() {
        let mut registry = Registry::<u8>::default();
        let first = registry.insert(1).unwrap();
        let second = registry.insert(2).unwrap();
        let third = registry.insert(3).unwrap();
        let ids = [first, second, third];
        assert_eq!(active_after_close(&ids, Some(ids[1]), ids[1]), Some(ids[2]));
        assert_eq!(active_after_close(&ids, Some(ids[2]), ids[2]), Some(ids[1]));
        assert_eq!(active_after_close(&ids, Some(ids[0]), ids[1]), Some(ids[0]));
        assert_eq!(active_after_close(&ids, Some(ids[0]), ids[0]), Some(ids[1]));
        assert_eq!(active_after_close(&[ids[0]], Some(ids[0]), ids[0]), None);
        assert_eq!(active_after_close::<u8>(&[], None, 1), None);
    }

    #[test]
    fn cycle_wraps_in_both_directions() {
        let mut registry = Registry::<u8>::default();
        let first = registry.insert(1).unwrap();
        let second = registry.insert(2).unwrap();
        let third = registry.insert(3).unwrap();
        let ids = [first, second, third];
        assert_eq!(cycle_id(&ids, Some(ids[2]), false), Some(ids[0]));
        assert_eq!(cycle_id(&ids, Some(ids[0]), true), Some(ids[2]));
        assert_eq!(cycle_id::<u8>(&[], None, false), None);
        assert_eq!(cycle_id(&[ids[0]], Some(ids[0]), false), Some(ids[0]));
        assert_eq!(cycle_id(&[ids[0]], Some(ids[0]), true), Some(ids[0]));
        assert_eq!(cycle_id(&ids, None, false), Some(ids[0]));
        assert_eq!(cycle_id(&ids, None, true), Some(ids[2]));
    }

    #[test]
    fn hit_test_routes_body_and_close_to_the_same_document_id() {
        let mut registry = Registry::<u8>::default();
        let first = registry.insert(1).unwrap();
        let second = registry.insert(2).unwrap();
        let rects = [
            (
                first,
                Rect {
                    pos: DVec2 { x: 10.0, y: 4.0 },
                    size: DVec2 { x: 120.0, y: 32.0 },
                },
            ),
            (
                second,
                Rect {
                    pos: DVec2 { x: 130.0, y: 4.0 },
                    size: DVec2 { x: 120.0, y: 32.0 },
                },
            ),
        ];
        assert_eq!(
            tab_hit(rects, DVec2 { x: 40.0, y: 20.0 }, TAB_CLOSE_WIDTH),
            Some((first, false))
        );
        assert_eq!(
            tab_hit(rects, DVec2 { x: 118.0, y: 20.0 }, TAB_CLOSE_WIDTH),
            Some((first, true))
        );
        let mut document_registry = Registry::<DocumentSession>::default();
        let document_id = document_registry
            .insert(
                DocumentSession::new(
                    DocumentSource::from_path(PathBuf::from("hit-test.pdf")),
                    OpenDocument::Pdf {
                        document_id: PdfDocumentId::new(1),
                        page_count: 1,
                        page_ids: vec![PageId::new(1)],
                    },
                )
                .expect("test PDF session must be valid"),
            )
            .unwrap();
        assert_eq!(
            action_for_hit(document_id, false),
            DocumentTabsAction::Activate(document_id)
        );
        assert_eq!(
            action_for_hit(document_id, true),
            DocumentTabsAction::Close(document_id)
        );
        assert_eq!(
            tab_hit(rects, DVec2 { x: 260.0, y: 20.0 }, TAB_CLOSE_WIDTH),
            None
        );
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use crate::OpenDocument;
    use crate::document_runtime::{DocumentSession, DocumentSource};
    use pdf_core::{DocumentId as PdfDocumentId, PageId};
    use std::path::PathBuf;

    #[test]
    fn projection_disambiguates_duplicate_names_and_marks_active_id() {
        let mut registry = rudra_runtime::Registry::<DocumentSession>::default();
        let session = |path: &str| {
            DocumentSession::new(
                DocumentSource::from_path(PathBuf::from(path)),
                OpenDocument::Pdf {
                    document_id: PdfDocumentId::new(1),
                    page_count: 1,
                    page_ids: vec![PageId::new(1)],
                },
            )
            .expect("test PDF session must be valid")
        };
        let first = registry.insert(session("same.pdf")).unwrap();
        let second = registry.insert(session("same.pdf")).unwrap();

        let frames = project_document_tabs(registry.iter(), Some(second));
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].label, "same.pdf · 1");
        assert_eq!(frames[1].label, "same.pdf · 2");
        assert_eq!(frames[0].display_label, "same.pdf · 1");
        assert_eq!(frames[1].display_label, "same.pdf · 2");
        assert!(!frames[0].active);
        assert!(frames[1].active);
        assert_eq!(frames[0].id, first);
        assert_eq!(frames[1].id, second);
    }

    #[test]
    fn compact_caption_is_unicode_safe_and_preserves_suffix() {
        let label = "Требования к составлению сметной документации.docx · 2";
        let compact = compact_tab_label(label, TAB_DISPLAY_MAX_CHARS);

        assert_eq!(compact.chars().count(), TAB_DISPLAY_MAX_CHARS);
        assert!(compact.starts_with("Требования"));
        assert!(compact.contains('…'));
        assert!(compact.ends_with("docx · 2"));
        assert_eq!(
            compact_tab_label("Кратко.pdf", TAB_DISPLAY_MAX_CHARS),
            "Кратко.pdf"
        );
    }

    #[test]
    fn upstream_tab_live_ids_map_back_to_runtime_document_ids() {
        let mut registry = rudra_runtime::Registry::<DocumentSession>::default();
        let session = |path: &str| {
            DocumentSession::new(
                DocumentSource::from_path(PathBuf::from(path)),
                OpenDocument::Pdf {
                    document_id: PdfDocumentId::new(1),
                    page_count: 1,
                    page_ids: vec![PageId::new(1)],
                },
            )
            .expect("test PDF session must be valid")
        };
        let first = registry.insert(session("first.pdf")).unwrap();
        let second = registry.insert(session("second.pdf")).unwrap();
        let frames = project_document_tabs(registry.iter(), Some(second));

        assert_eq!(
            document_id_for_tab_live_id(&frames, tab_live_id(first)),
            Some(first)
        );
        assert_eq!(
            document_id_for_tab_live_id(&frames, tab_live_id(second)),
            Some(second)
        );
        assert_eq!(
            document_id_for_tab_live_id(
                &frames,
                LiveId::from_str_num("rudra-office-document-tab", 999),
            ),
            None
        );
    }

    #[test]
    fn resize_reveal_uses_one_neutral_draw_without_changing_the_active_frame() {
        let mut registry = rudra_runtime::Registry::<DocumentSession>::default();
        let session = |path: &str| {
            DocumentSession::new(
                DocumentSource::from_path(PathBuf::from(path)),
                OpenDocument::Pdf {
                    document_id: PdfDocumentId::new(1),
                    page_count: 1,
                    page_ids: vec![PageId::new(1)],
                },
            )
            .expect("test PDF session must be valid")
        };
        let first = registry.insert(session("first.pdf")).unwrap();
        let second = registry.insert(session("second.pdf")).unwrap();
        let frames = project_document_tabs(registry.iter(), Some(second));

        assert_eq!(active_index_for_draw(&frames, false), Some(1));
        assert_eq!(active_index_for_draw(&frames, true), None);
        assert!(frames.iter().any(|tab| tab.id == second && tab.active));
        assert!(frames.iter().any(|tab| tab.id == first && !tab.active));
    }
}
/// Exact visual bounds for one document tab and its close affordance.
///
/// This is a semantic projection only: the tab strip remains the sole owner of
/// drawing and hit-testing.
#[derive(Clone, Debug)]
pub(crate) struct DocumentTabAccessibilityFrame {
    pub(crate) id: DocumentId,
    pub(crate) label: String,
    pub(crate) active: bool,
    pub(crate) bounds: Rect,
    pub(crate) close_bounds: Rect,
}

fn close_rect(rect: Rect) -> Rect {
    let width = TAB_CLOSE_WIDTH.min(rect.size.x.max(0.0));
    Rect {
        pos: dvec2(rect.pos.x + rect.size.x - width, rect.pos.y),
        size: dvec2(width, rect.size.y.max(0.0)),
    }
}

impl DocumentTabStrip {
    /// Returns the actual tab rectangles produced by the upstream `TabBar`.
    pub(crate) fn accessibility_tabs(&self, cx: &Cx) -> Vec<DocumentTabAccessibilityFrame> {
        self.frame
            .iter()
            .filter_map(|tab| {
                self.tab_bar
                    .tab_rect(cx, tab_live_id(tab.id))
                    .map(|bounds| DocumentTabAccessibilityFrame {
                        id: tab.id,
                        label: tab.label.clone(),
                        active: tab.active,
                        close_bounds: close_rect(bounds),
                        bounds,
                    })
            })
            .collect()
    }
}
