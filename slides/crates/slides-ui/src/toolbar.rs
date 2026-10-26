//! Renderer-neutral toolbar composition for Rudra Slides.
//!
//! `ToolbarView` is a deterministic, read-only projection derived from
//! [`EditorUi`]. It does not mutate UI state, does not depend on render plans,
//! and does not introduce new actions. Controls reference only existing
//! [`UiAction`] values and expose stable metadata for a host renderer to
//! consume.
//!
//! v1 tab contract:
//! - `Home`: slide creation, deletion, and duplication.
//! - `Insert`: optional unsupported disabled insert affordance. No properties
//!   toggle here.
//! - `Design`: visible disabled transition affordance only. Notes live under
//!   `View`.
//! - `Slideshow`: playback-only start/stop controls.
//! - `View`: zoom and panel toggles for properties and notes.

use crate::{RibbonTab, SlideshowMode, SlideshowStart, UiAction};

/// Stable identifier for a toolbar tab.
pub type ToolbarTabId = &'static str;
/// Stable identifier for a control group.
pub type ToolbarGroupId = &'static str;
/// Stable identifier for a control.
pub type ToolbarControlId = &'static str;

/// Visible control kind for the toolbar model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarControlKind {
    /// Button control.
    #[default]
    Button,
    /// Toggle control.
    Toggle,
    /// Separator.
    Separator,
    /// Label control.
    Label,
}

/// Reason why a visible toolbar control is disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarDisabledReason {
    /// No disablement.
    #[default]
    None,
    /// The UI snapshot is read-only.
    ReadOnly,
    /// A slideshow is currently presenting.
    PresentationActive,
    /// No slideshow is currently running.
    NotPlaying,
    /// The presentation has no slides.
    EmptyDeck,
    /// Only one slide remains and deletion is unavailable.
    SingleSlide,
    /// The capability is not supported in the current toolbar model.
    MissingCapability,
}

/// One toolbar control.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarControl {
    id: ToolbarControlId,
    kind: ToolbarControlKind,
    label: &'static str,
    accessibility_label: &'static str,
    visible: bool,
    disabled: bool,
    disabled_reason: Option<ToolbarDisabledReason>,
    action: Option<UiAction>,
}

impl ToolbarControl {
    /// Stable control identifier.
    pub fn id(&self) -> ToolbarControlId {
        self.id
    }

    /// Control kind.
    pub fn kind(&self) -> ToolbarControlKind {
        self.kind
    }

    /// Primary label.
    pub fn label(&self) -> &'static str {
        self.label
    }

    /// Accessibility label.
    pub fn accessibility_label(&self) -> &'static str {
        self.accessibility_label
    }

    /// Whether the control is visible.
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Whether the control is disabled.
    pub fn disabled(&self) -> bool {
        self.disabled
    }

    /// Whether the control is enabled.
    pub fn is_enabled(&self) -> bool {
        !self.disabled
    }

    /// Why the control is disabled, if any.
    pub fn disabled_reason(&self) -> Option<ToolbarDisabledReason> {
        self.disabled_reason
    }

    /// Existing [`UiAction`] associated with the control, if any.
    pub fn action(&self) -> Option<&UiAction> {
        if self.disabled {
            None
        } else {
            self.action.as_ref()
        }
    }
}

/// One group of controls within a tab.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarGroup {
    id: ToolbarGroupId,
    label: &'static str,
    accessibility_label: &'static str,
    controls: Vec<ToolbarControl>,
}

impl ToolbarGroup {
    /// Stable group identifier.
    pub fn id(&self) -> ToolbarGroupId {
        self.id
    }

    /// Group label.
    pub fn label(&self) -> &'static str {
        self.label
    }

    /// Accessibility label for the group.
    pub fn accessibility_label(&self) -> &'static str {
        self.accessibility_label
    }

    /// Controls in declaration order.
    pub fn controls(&self) -> &[ToolbarControl] {
        &self.controls
    }
}

/// One toolbar tab.
///
/// Tabs keep the fixed v1 set: `Home`, `Insert`, `Design`, `Slideshow`, `View`.
/// `Design` contains only a disabled transitions affordance, and `Slideshow`
/// is playback-only.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarTab {
    id: ToolbarTabId,
    label: &'static str,
    accessibility_label: &'static str,
    groups: Vec<ToolbarGroup>,
}

impl ToolbarTab {
    /// Stable tab identifier.
    pub fn id(&self) -> ToolbarTabId {
        self.id
    }

    /// Tab label.
    pub fn label(&self) -> &'static str {
        self.label
    }

    /// Accessibility label for the tab.
    pub fn accessibility_label(&self) -> &'static str {
        self.accessibility_label
    }

    /// Groups in declaration order.
    pub fn groups(&self) -> &[ToolbarGroup] {
        &self.groups
    }
}

/// Deterministic toolbar view projected from [`EditorUi`].
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarView {
    tabs: Vec<ToolbarTab>,
    active_tab: ToolbarTabId,
}

impl ToolbarView {
    /// Tabs in stable declaration order.
    pub fn tabs(&self) -> &[ToolbarTab] {
        &self.tabs
    }

    /// Active tab identifier mirrored from [`RibbonState::active_tab`].
    pub fn active_tab(&self) -> ToolbarTabId {
        self.active_tab
    }

    /// Flattened controls in declaration order.
    pub fn controls(&self) -> Vec<&ToolbarControl> {
        self.tabs
            .iter()
            .flat_map(|tab| tab.groups.iter())
            .flat_map(|group| group.controls.iter())
            .collect()
    }
}

/// Projects a [`ToolbarView`] from the current editor UI state.
///
/// The projection is pure: it reads visibility, slideshow mode, read-only
/// state, zoom state, and active ribbon tab without mutating `ui`.
pub fn project_toolbar(ui: &crate::EditorUi) -> ToolbarView {
    let read_only = ui.read_only();
    let presenting = matches!(ui.slideshow.mode, SlideshowMode::Presenting);
    let slide_count = ui.thumbnails.len();
    let active_tab = match ui.ribbon.active_tab {
        RibbonTab::Home => "home",
        RibbonTab::Insert => "insert",
        RibbonTab::Design => "design",
        RibbonTab::Slideshow => "slideshow",
        RibbonTab::View => "view",
    };

    let home_tab = ToolbarTab {
        id: "home",
        label: "Home",
        accessibility_label: "Home tab",
        groups: vec![ToolbarGroup {
            id: "slide-actions",
            label: "Slide",
            accessibility_label: "Slide actions",
            controls: vec![
                ToolbarControl {
                    id: "new-slide",
                    kind: ToolbarControlKind::Button,
                    label: "New Slide",
                    accessibility_label: "Create a new slide",
                    visible: true,
                    disabled: read_only || presenting,
                    disabled_reason: if read_only {
                        Some(ToolbarDisabledReason::ReadOnly)
                    } else if presenting {
                        Some(ToolbarDisabledReason::PresentationActive)
                    } else {
                        None
                    },
                    action: Some(UiAction::NewSlide),
                },
                ToolbarControl {
                    id: "delete-slide",
                    kind: ToolbarControlKind::Button,
                    label: "Delete",
                    accessibility_label: "Delete the current slide",
                    visible: true,
                    disabled: read_only || presenting || slide_count <= 1,
                    disabled_reason: if read_only {
                        Some(ToolbarDisabledReason::ReadOnly)
                    } else if presenting {
                        Some(ToolbarDisabledReason::PresentationActive)
                    } else if slide_count <= 1 {
                        Some(ToolbarDisabledReason::SingleSlide)
                    } else {
                        None
                    },
                    action: Some(UiAction::DeleteCurrentSlide),
                },
                ToolbarControl {
                    id: "duplicate-slide",
                    kind: ToolbarControlKind::Button,
                    label: "Duplicate",
                    accessibility_label: "Duplicate the current slide",
                    visible: true,
                    disabled: read_only || presenting || slide_count == 0,
                    disabled_reason: if read_only {
                        Some(ToolbarDisabledReason::ReadOnly)
                    } else if presenting {
                        Some(ToolbarDisabledReason::PresentationActive)
                    } else if slide_count == 0 {
                        Some(ToolbarDisabledReason::EmptyDeck)
                    } else {
                        None
                    },
                    action: Some(UiAction::DuplicateCurrentSlide),
                },
            ],
        }],
    };

    let insert_tab = ToolbarTab {
        id: "insert",
        label: "Insert",
        accessibility_label: "Insert tab",
        groups: vec![ToolbarGroup {
            id: "insert-actions",
            label: "Insert",
            accessibility_label: "Insert actions",
            controls: vec![ToolbarControl {
                id: "insert-object",
                kind: ToolbarControlKind::Button,
                label: "Insert Object",
                accessibility_label: "Missing capability: insert object",
                visible: true,
                disabled: true,
                disabled_reason: Some(ToolbarDisabledReason::MissingCapability),
                action: None,
            }],
        }],
    };

    let design_tab = ToolbarTab {
        id: "design",
        label: "Design",
        accessibility_label: "Design tab",
        groups: vec![ToolbarGroup {
            id: "transitions",
            label: "Transitions",
            accessibility_label: "Transitions",
            controls: vec![ToolbarControl {
                id: "transitions",
                kind: ToolbarControlKind::Button,
                label: "Transitions",
                accessibility_label: "Missing capability: transitions",
                visible: true,
                disabled: true,
                disabled_reason: Some(ToolbarDisabledReason::MissingCapability),
                action: None,
            }],
        }],
    };

    let slideshow_tab = ToolbarTab {
        id: "slideshow",
        label: "Slideshow",
        accessibility_label: "Slideshow tab",
        groups: vec![ToolbarGroup {
            id: "playback",
            label: "Playback",
            accessibility_label: "Playback controls",
            controls: vec![
                ToolbarControl {
                    id: "start-slideshow",
                    kind: ToolbarControlKind::Button,
                    label: "Start",
                    accessibility_label: "Start slideshow",
                    visible: true,
                    disabled: presenting || slide_count == 0,
                    disabled_reason: if presenting {
                        Some(ToolbarDisabledReason::PresentationActive)
                    } else if slide_count == 0 {
                        Some(ToolbarDisabledReason::EmptyDeck)
                    } else {
                        None
                    },
                    action: Some(UiAction::StartSlideshow(SlideshowStart::Current)),
                },
                ToolbarControl {
                    id: "stop-slideshow",
                    kind: ToolbarControlKind::Button,
                    label: "Stop",
                    accessibility_label: "Stop slideshow",
                    visible: true,
                    disabled: !presenting,
                    disabled_reason: if presenting {
                        None
                    } else {
                        Some(ToolbarDisabledReason::NotPlaying)
                    },
                    action: Some(UiAction::StopSlideshow),
                },
            ],
        }],
    };

    let view_tab = ToolbarTab {
        id: "view",
        label: "View",
        accessibility_label: "View tab",
        groups: vec![
            ToolbarGroup {
                id: "zoom",
                label: "Zoom",
                accessibility_label: "Zoom controls",
                controls: vec![
                    ToolbarControl {
                        id: "zoom-out",
                        kind: ToolbarControlKind::Button,
                        label: "Zoom Out",
                        accessibility_label: "Zoom out",
                        visible: true,
                        disabled: false,
                        disabled_reason: None,
                        action: Some(UiAction::ZoomOut),
                    },
                    ToolbarControl {
                        id: "zoom-in",
                        kind: ToolbarControlKind::Button,
                        label: "Zoom In",
                        accessibility_label: "Zoom in",
                        visible: true,
                        disabled: false,
                        disabled_reason: None,
                        action: Some(UiAction::ZoomIn),
                    },
                    ToolbarControl {
                        id: "reset-zoom",
                        kind: ToolbarControlKind::Button,
                        label: "Reset Zoom",
                        accessibility_label: "Reset zoom to default",
                        visible: true,
                        disabled: false,
                        disabled_reason: None,
                        action: Some(UiAction::ResetZoom),
                    },
                ],
            },
            ToolbarGroup {
                id: "panels",
                label: "Panels",
                accessibility_label: "Panel toggles",
                controls: vec![
                    ToolbarControl {
                        id: "toggle-properties",
                        kind: ToolbarControlKind::Toggle,
                        label: "Properties",
                        accessibility_label: "Toggle properties panel",
                        visible: true,
                        disabled: false,
                        disabled_reason: None,
                        action: Some(UiAction::ToggleProperties),
                    },
                    ToolbarControl {
                        id: "toggle-notes",
                        kind: ToolbarControlKind::Toggle,
                        label: "Notes",
                        accessibility_label: "Toggle notes panel",
                        visible: true,
                        disabled: false,
                        disabled_reason: None,
                        action: Some(UiAction::ToggleNotes),
                    },
                    ToolbarControl {
                        id: "toggle-fullscreen",
                        kind: ToolbarControlKind::Toggle,
                        label: "Fullscreen",
                        accessibility_label: "Toggle fullscreen",
                        visible: true,
                        disabled: false,
                        disabled_reason: None,
                        action: Some(UiAction::ToggleFullscreen),
                    },
                ],
            },
        ],
    };

    ToolbarView {
        tabs: vec![home_tab, insert_tab, design_tab, slideshow_tab, view_tab],
        active_tab,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slides_api::{Presentation, Slide, SlideId};

    fn document(count: usize) -> Presentation {
        Presentation {
            slides: (0..count)
                .map(|index| Slide::new(SlideId::new(index as u64 + 1)))
                .collect(),
            ..Presentation::default()
        }
    }

    #[test]
    fn projection_is_deterministic() {
        let ui = crate::EditorUi::from_document(&document(3));
        assert_eq!(project_toolbar(&ui), project_toolbar(&ui));
    }

    #[test]
    fn projection_does_not_mutate_editor_ui() {
        let ui = crate::EditorUi::from_document(&document(2));
        let ui_before = ui.clone();
        let _toolbar = project_toolbar(&ui);
        assert_eq!(ui, ui_before);
    }

    #[test]
    fn tab_order_and_active_tab_mirror_ribbon_state() {
        let mut ui = crate::EditorUi::from_document(&document(1));
        ui.ribbon.active_tab = crate::RibbonTab::Design;
        let toolbar = project_toolbar(&ui);
        assert_eq!(
            toolbar
                .tabs()
                .iter()
                .map(|tab| tab.id())
                .collect::<Vec<_>>(),
            vec!["home", "insert", "design", "slideshow", "view"]
        );
        assert_eq!(toolbar.active_tab(), "design");
    }

    #[test]
    fn all_ids_are_ascii_lowercase_alnum_hyphen() {
        let toolbar = project_toolbar(&crate::EditorUi::from_document(&document(2)));
        for tab in toolbar.tabs() {
            assert!(!tab.id().is_empty());
            assert!(
                tab.id()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            );
            assert!(!tab.label().is_empty());
            assert!(!tab.accessibility_label().is_empty());
            for group in tab.groups() {
                assert!(!group.id().is_empty());
                assert!(
                    group
                        .id()
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                );
                assert!(!group.label().is_empty());
                assert!(!group.accessibility_label().is_empty());
                for control in group.controls() {
                    assert!(!control.id().is_empty());
                    assert!(
                        control
                            .id()
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                    );
                    assert!(!control.label().is_empty());
                    assert!(!control.accessibility_label().is_empty());
                }
            }
        }
    }

    #[test]
    fn all_disabled_controls_have_reason() {
        let cases = [
            crate::EditorUi::from_document(&document(2)),
            {
                let mut ui = crate::EditorUi::from_document(&document(2));
                ui.read_only = true;
                ui
            },
            {
                let mut ui = crate::EditorUi::from_document(&document(2));
                ui.slideshow.mode = crate::SlideshowMode::Presenting;
                ui
            },
            crate::EditorUi::from_document(&document(0)),
            crate::EditorUi::from_document(&document(1)),
        ];

        for ui in cases {
            let toolbar = project_toolbar(&ui);
            for control in toolbar.controls() {
                if control.disabled() {
                    assert!(
                        control.disabled_reason().is_some(),
                        "disabled control {} in state read_only={} presenting={:?} slide_count={} has no reason",
                        control.id(),
                        ui.read_only(),
                        ui.slideshow.mode,
                        ui.thumbnails.len()
                    );
                } else {
                    assert_eq!(
                        control.disabled_reason(),
                        None,
                        "enabled control {} should not have a disabled reason",
                        control.id()
                    );
                }
            }
        }
    }

    #[test]
    fn design_transitions_disabled_missing_capability_not_in_slideshow() {
        let toolbar = project_toolbar(&crate::EditorUi::from_document(&document(1)));
        let design_tab = toolbar
            .tabs()
            .iter()
            .find(|tab| tab.id() == "design")
            .unwrap();
        let transitions = design_tab
            .groups()
            .iter()
            .flat_map(|group| group.controls())
            .find(|control| control.id() == "transitions")
            .unwrap();
        assert!(transitions.visible());
        assert!(transitions.disabled());
        assert_eq!(
            transitions.disabled_reason(),
            Some(ToolbarDisabledReason::MissingCapability)
        );
        assert!(transitions.action().is_none());

        let slideshow_tab = toolbar
            .tabs()
            .iter()
            .find(|tab| tab.id() == "slideshow")
            .unwrap();
        assert!(
            slideshow_tab
                .groups()
                .iter()
                .flat_map(|group| group.controls())
                .all(|control| control.id() != "transitions")
        );
    }

    #[test]
    fn read_only_does_not_disable_local_view_controls() {
        let mut ui = crate::EditorUi::from_document(&document(2));
        ui.read_only = true;
        let toolbar = project_toolbar(&ui);
        let controls = toolbar.controls();

        let core_ids = ["new-slide", "delete-slide", "duplicate-slide"];
        let local_ids = [
            "zoom-in",
            "zoom-out",
            "reset-zoom",
            "toggle-properties",
            "toggle-notes",
            "toggle-fullscreen",
        ];

        for id in core_ids {
            let control = controls.iter().find(|c| *c.id() == *id).unwrap();
            assert!(control.disabled(), "{} should be disabled in read-only", id);
        }
        for id in local_ids {
            let control = controls.iter().find(|c| *c.id() == *id).unwrap();
            assert!(
                !control.disabled(),
                "{} should remain enabled in read-only",
                id
            );
        }
    }

    #[test]
    fn empty_deck_and_duplicate_gating() {
        let ui = project_toolbar(&crate::EditorUi::from_document(&document(0)));
        let controls = ui.controls();
        let start = controls
            .iter()
            .find(|c| c.id() == "start-slideshow")
            .unwrap();
        let duplicate = controls
            .iter()
            .find(|c| c.id() == "duplicate-slide")
            .unwrap();
        assert!(start.disabled());
        assert_eq!(
            start.disabled_reason(),
            Some(ToolbarDisabledReason::EmptyDeck)
        );
        assert!(duplicate.disabled());
        assert_eq!(
            duplicate.disabled_reason(),
            Some(ToolbarDisabledReason::EmptyDeck)
        );

        let single = project_toolbar(&crate::EditorUi::from_document(&document(1)));
        let controls = single.controls();
        let delete = controls.iter().find(|c| c.id() == "delete-slide").unwrap();
        assert!(delete.disabled());
        assert_eq!(
            delete.disabled_reason(),
            Some(ToolbarDisabledReason::SingleSlide)
        );
    }

    #[test]
    fn slideshow_gating() {
        let mut ui = crate::EditorUi::from_document(&document(2));
        let toolbar = project_toolbar(&ui);
        let controls = toolbar.controls();
        let start = controls
            .iter()
            .find(|c| c.id() == "start-slideshow")
            .unwrap();
        let stop = controls
            .iter()
            .find(|c| c.id() == "stop-slideshow")
            .unwrap();
        assert!(!start.disabled());
        assert!(stop.disabled());
        assert_eq!(
            stop.disabled_reason(),
            Some(ToolbarDisabledReason::NotPlaying)
        );

        ui.slideshow.mode = crate::SlideshowMode::Presenting;
        let toolbar = project_toolbar(&ui);
        let controls = toolbar.controls();
        let start = controls
            .iter()
            .find(|c| c.id() == "start-slideshow")
            .unwrap();
        let stop = controls
            .iter()
            .find(|c| c.id() == "stop-slideshow")
            .unwrap();
        assert!(start.disabled());
        assert_eq!(
            start.disabled_reason(),
            Some(ToolbarDisabledReason::PresentationActive)
        );
        assert!(!stop.disabled());
    }

    #[test]
    fn public_getters_cover_projection() {
        let ui = crate::EditorUi::from_document(&document(3));
        let toolbar = project_toolbar(&ui);

        assert_eq!(toolbar.active_tab(), "home");
        assert_eq!(toolbar.tabs().len(), 5);

        let home = toolbar
            .tabs()
            .iter()
            .find(|tab| tab.id() == "home")
            .expect("home tab exists");
        assert_eq!(home.label(), "Home");
        assert_eq!(home.accessibility_label(), "Home tab");
        assert_eq!(home.groups().len(), 1);

        let slide_group = &home.groups()[0];
        assert_eq!(slide_group.id(), "slide-actions");
        assert_eq!(slide_group.label(), "Slide");
        assert_eq!(slide_group.accessibility_label(), "Slide actions");
        assert_eq!(slide_group.controls().len(), 3);

        let new_slide = &slide_group.controls()[0];
        assert_eq!(new_slide.id(), "new-slide");
        assert_eq!(new_slide.kind(), ToolbarControlKind::Button);
        assert_eq!(new_slide.label(), "New Slide");
        assert_eq!(new_slide.accessibility_label(), "Create a new slide");
        assert!(new_slide.is_enabled());
        assert_eq!(new_slide.disabled_reason(), None);
        assert_eq!(new_slide.action(), Some(&UiAction::NewSlide));

        let design = toolbar
            .tabs()
            .iter()
            .find(|tab| tab.id() == "design")
            .expect("design tab exists");
        let transitions = design
            .groups()
            .iter()
            .flat_map(|group| group.controls())
            .find(|control| control.id() == "transitions")
            .expect("transitions control exists");
        assert_eq!(transitions.action(), None);
        assert_eq!(
            transitions.disabled_reason(),
            Some(ToolbarDisabledReason::MissingCapability)
        );
    }

    #[test]
    fn toolbar_does_not_depend_on_render_plan_types() {
        let _ = project_toolbar(&crate::EditorUi::from_document(&document(1)));
    }

    #[test]
    fn disabled_controls_have_no_action() {
        let mut ui = crate::EditorUi::from_document(&document(1));
        ui.read_only = true;
        let toolbar = project_toolbar(&ui);
        let controls = toolbar.controls();
        let new_slide = controls.iter().find(|c| c.id() == "new-slide").unwrap();
        assert!(new_slide.disabled());
        assert_eq!(
            new_slide.disabled_reason(),
            Some(ToolbarDisabledReason::ReadOnly)
        );
        assert_eq!(new_slide.action(), None);

        let mut ui = crate::EditorUi::from_document(&document(1));
        ui.slideshow.mode = crate::SlideshowMode::Presenting;
        let toolbar = project_toolbar(&ui);
        let controls = toolbar.controls();
        let new_slide = controls.iter().find(|c| c.id() == "new-slide").unwrap();
        assert!(new_slide.disabled());
        assert_eq!(
            new_slide.disabled_reason(),
            Some(ToolbarDisabledReason::PresentationActive)
        );
        assert_eq!(new_slide.action(), None);
    }

    #[test]
    fn single_slide_delete_is_disabled_without_action() {
        let toolbar = project_toolbar(&crate::EditorUi::from_document(&document(1)));
        let controls = toolbar.controls();
        let delete = controls.iter().find(|c| c.id() == "delete-slide").unwrap();
        assert!(delete.disabled());
        assert_eq!(
            delete.disabled_reason(),
            Some(ToolbarDisabledReason::SingleSlide)
        );
        assert_eq!(delete.action(), None);
    }

    #[test]
    fn empty_deck_duplicate_and_start_are_disabled_without_action() {
        let toolbar = project_toolbar(&crate::EditorUi::from_document(&document(0)));
        let controls = toolbar.controls();
        let duplicate = controls
            .iter()
            .find(|c| c.id() == "duplicate-slide")
            .unwrap();
        let start = controls
            .iter()
            .find(|c| c.id() == "start-slideshow")
            .unwrap();
        assert!(duplicate.disabled());
        assert_eq!(
            duplicate.disabled_reason(),
            Some(ToolbarDisabledReason::EmptyDeck)
        );
        assert_eq!(duplicate.action(), None);
        assert!(start.disabled());
        assert_eq!(
            start.disabled_reason(),
            Some(ToolbarDisabledReason::EmptyDeck)
        );
        assert_eq!(start.action(), None);
    }

    #[test]
    fn stop_slideshow_disabled_when_not_playing() {
        let toolbar = project_toolbar(&crate::EditorUi::from_document(&document(2)));
        let controls = toolbar.controls();
        let stop = controls
            .iter()
            .find(|c| c.id() == "stop-slideshow")
            .unwrap();
        assert!(stop.disabled());
        assert_eq!(
            stop.disabled_reason(),
            Some(ToolbarDisabledReason::NotPlaying)
        );
        assert_eq!(stop.action(), None);
    }

    #[test]
    fn enabled_controls_expose_exact_action() {
        let ui = crate::EditorUi::from_document(&document(2));
        let toolbar = project_toolbar(&ui);
        let controls = toolbar.controls();
        let new_slide = controls.iter().find(|c| c.id() == "new-slide").unwrap();
        let delete = controls.iter().find(|c| c.id() == "delete-slide").unwrap();
        let duplicate = controls
            .iter()
            .find(|c| c.id() == "duplicate-slide")
            .unwrap();
        let start = controls
            .iter()
            .find(|c| c.id() == "start-slideshow")
            .unwrap();
        assert_eq!(new_slide.action(), Some(&UiAction::NewSlide));
        assert_eq!(delete.action(), Some(&UiAction::DeleteCurrentSlide));
        assert_eq!(duplicate.action(), Some(&UiAction::DuplicateCurrentSlide));
        assert_eq!(
            start.action(),
            Some(&UiAction::StartSlideshow(crate::SlideshowStart::Current))
        );

        let mut presenting = crate::EditorUi::from_document(&document(2));
        presenting.slideshow.mode = crate::SlideshowMode::Presenting;
        let toolbar = project_toolbar(&presenting);
        let controls = toolbar.controls();
        let stop = controls
            .iter()
            .find(|c| c.id() == "stop-slideshow")
            .unwrap();
        assert_eq!(stop.action(), Some(&UiAction::StopSlideshow));
    }
}
