#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use writer_api::core::{
    Alignment, BlockKind, Command, DocumentFormat, HeadingLevel, LineSpacing, ParagraphStylePatch,
    Rgba8, SearchQuery, Selection, TextContainerId, TextStylePatch,
};
use writer_api::layout::{LayoutMode, LayoutSnapshot};
use writer_api::{ApiError, ExportedDocument, WriterApi};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Pages,
    Continuous,
}

impl From<LayoutMode> for ViewMode {
    fn from(value: LayoutMode) -> Self {
        match value {
            LayoutMode::Pages => Self::Pages,
            LayoutMode::Continuous => Self::Continuous,
        }
    }
}

impl From<ViewMode> for LayoutMode {
    fn from(value: ViewMode) -> Self {
        match value {
            ViewMode::Pages => Self::Pages,
            ViewMode::Continuous => Self::Continuous,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadingEntry {
    pub block_id: u64,
    pub level: u8,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchEntry {
    pub block_id: u64,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageEntry {
    pub index: usize,
    pub block_count: usize,
    pub height_half_points: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewState {
    pub mode: ViewMode,
    pub zoom_percent: u16,
    pub selected_block: Option<u64>,
    pub headings: Vec<HeadingEntry>,
    pub matches: Vec<MatchEntry>,
    pub pages: Vec<PageEntry>,
    pub layout: Option<LayoutSnapshot>,
    pub status: String,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            mode: ViewMode::Pages,
            zoom_percent: 100,
            selected_block: None,
            headings: Vec::new(),
            matches: Vec::new(),
            pages: Vec::new(),
            layout: None,
            status: "Ready".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ToolbarValue<T> {
    Known(T),
    Mixed,
    #[default]
    Unavailable,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ToolbarField<T> {
    pub value: ToolbarValue<T>,
    pub enabled: bool,
}

impl<T: fmt::Debug> fmt::Debug for ToolbarField<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolbarField")
            .field("value", &self.value)
            .field("enabled", &self.enabled)
            .finish()
    }
}

pub struct ToolbarState {
    pub size_half_points: ToolbarField<u16>,
    pub indent_left_half_points: ToolbarField<i32>,
    pub indent_right_half_points: ToolbarField<i32>,
    pub first_line_indent_half_points: ToolbarField<i32>,
    pub font_family: ToolbarField<Option<String>>,
    pub bold: ToolbarField<bool>,
    pub italic: ToolbarField<bool>,
    pub underline: ToolbarField<bool>,
    pub strikethrough: ToolbarField<bool>,
    pub foreground: ToolbarField<Option<Rgba8>>,
    pub background: ToolbarField<Option<Rgba8>>,
    pub alignment: ToolbarField<Alignment>,
    pub line_spacing: ToolbarField<LineSpacing>,
    pub block_kind: ToolbarField<BlockKind>,
    pub lists_enabled: bool,
}

impl Clone for ToolbarState {
    fn clone(&self) -> Self {
        Self {
            size_half_points: self.size_half_points.clone(),
            indent_left_half_points: self.indent_left_half_points.clone(),
            indent_right_half_points: self.indent_right_half_points.clone(),
            first_line_indent_half_points: self.first_line_indent_half_points.clone(),
            font_family: self.font_family.clone(),
            bold: self.bold.clone(),
            italic: self.italic.clone(),
            underline: self.underline.clone(),
            strikethrough: self.strikethrough.clone(),
            foreground: self.foreground.clone(),
            background: self.background.clone(),
            alignment: self.alignment.clone(),
            line_spacing: self.line_spacing.clone(),
            block_kind: self.block_kind.clone(),
            lists_enabled: self.lists_enabled,
        }
    }
}

impl PartialEq for ToolbarState {
    fn eq(&self, other: &Self) -> bool {
        self.size_half_points == other.size_half_points
            && self.indent_left_half_points == other.indent_left_half_points
            && self.indent_right_half_points == other.indent_right_half_points
            && self.first_line_indent_half_points == other.first_line_indent_half_points
            && self.font_family == other.font_family
            && self.bold == other.bold
            && self.italic == other.italic
            && self.underline == other.underline
            && self.strikethrough == other.strikethrough
            && self.foreground == other.foreground
            && self.background == other.background
            && self.alignment == other.alignment
            && self.line_spacing == other.line_spacing
            && self.block_kind == other.block_kind
            && self.lists_enabled == other.lists_enabled
    }
}

impl Eq for ToolbarState {}

impl fmt::Debug for ToolbarState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolbarState")
            .field("size_half_points", &self.size_half_points)
            .field("indent_left_half_points", &self.indent_left_half_points)
            .field("indent_right_half_points", &self.indent_right_half_points)
            .field(
                "first_line_indent_half_points",
                &self.first_line_indent_half_points,
            )
            .field("font_family", &self.font_family)
            .field("bold", &self.bold)
            .field("italic", &self.italic)
            .field("underline", &self.underline)
            .field("strikethrough", &self.strikethrough)
            .field("foreground", &self.foreground)
            .field("background", &self.background)
            .field("alignment", &self.alignment)
            .field("line_spacing", &self.line_spacing)
            .field("block_kind", &self.block_kind)
            .field("lists_enabled", &self.lists_enabled)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    SetFontFamily(Option<String>),
    SetSizeHalfPoints(u16),
    SetBold(bool),
    SetItalic(bool),
    SetUnderline(bool),
    SetStrikethrough(bool),
    SetForeground(Option<Rgba8>),
    SetBackground(Option<Rgba8>),
    SetAlignment(Alignment),
    SetLineSpacing(LineSpacing),
    SetIndents {
        left_half_points: i32,
        right_half_points: i32,
        first_line_half_points: i32,
    },
    SetHeading(Option<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarApplyStatus {
    Applied,
    Unchanged,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarGroup {
    Character,
    Paragraph,
    Styles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarActionKind {
    FontFamily,
    FontSize,
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Foreground,
    Background,
    AlignLeft,
    AlignCenter,
    AlignRight,
    AlignJustified,
    LineSpacing,
    Indents,
    ParagraphStyle,
    Heading1,
    Heading2,
    Heading3,
    Heading4,
    Heading5,
    Heading6,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarControl {
    pub group: ToolbarGroup,
    pub action: ToolbarActionKind,
    pub label: &'static str,
    pub enabled: bool,
    pub active: bool,
    pub indeterminate: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarRenderGroup {
    pub group: ToolbarGroup,
    pub controls: Vec<ToolbarControl>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarRenderModel {
    pub groups: Vec<ToolbarRenderGroup>,
}

#[derive(Debug, PartialEq)]
pub enum UiError {
    Api(ApiError),
    InvalidZoom,
    InvalidToolbarSize,
    InvalidHeadingLevel,
    InvalidSelection,
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Api(error) => error.fmt(f),
            Self::InvalidZoom => f.write_str("zoom must be between 10% and 500%"),
            Self::InvalidToolbarSize => {
                f.write_str("font size must be between 1 and 16384 half-points")
            }
            Self::InvalidHeadingLevel => f.write_str("heading level must be between 1 and 6"),
            Self::InvalidSelection => f.write_str("invalid selection"),
        }
    }
}

impl Error for UiError {}
impl From<ApiError> for UiError {
    fn from(error: ApiError) -> Self {
        Self::Api(error)
    }
}

pub struct WriterViewModel {
    api: WriterApi,
    state: ViewState,
}

impl Default for WriterViewModel {
    fn default() -> Self {
        Self::new()
    }
}

impl WriterViewModel {
    pub fn new() -> Self {
        let mut view = Self {
            api: WriterApi::new(),
            state: ViewState::default(),
        };
        view.refresh();
        view
    }

    pub fn api(&self) -> &WriterApi {
        &self.api
    }
    pub fn api_mut(&mut self) -> &mut WriterApi {
        &mut self.api
    }
    pub fn state(&self) -> &ViewState {
        &self.state
    }
    pub fn snapshot(&self) -> writer_api::core::DocumentSnapshot {
        self.api.document()
    }

    pub fn docx_layout(&self) -> &writer_api::DocxLayoutMetadata {
        self.api.docx_layout()
    }

    pub fn set_header_distance_half_points(&mut self, value: u32) -> bool {
        let changed = self.api.set_header_distance_half_points(value);
        if changed {
            self.state.status = "Header position changed".to_string();
            self.refresh();
        }
        changed
    }

    pub fn set_footer_distance_half_points(&mut self, value: u32) -> bool {
        let changed = self.api.set_footer_distance_half_points(value);
        if changed {
            self.state.status = "Footer position changed".to_string();
            self.refresh();
        }
        changed
    }

    pub fn docx_projection(&self) -> &writer_api::DocxProjectionDiagnostics {
        self.api.docx_projection()
    }

    pub fn insert_text(&mut self, text: impl Into<String>) -> Result<(), UiError> {
        self.api.insert_text(text)?;
        self.state.matches.clear();
        self.state.status = "Text inserted".to_string();
        self.refresh();
        Ok(())
    }

    pub fn execute(&mut self, command: writer_api::core::Command) -> Result<(), UiError> {
        self.api.execute(command)?;
        self.state.matches.clear();
        self.state.status = "Document updated".to_string();
        self.refresh();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<bool, UiError> {
        let changed = self.api.undo()?;
        if changed {
            self.state.matches.clear();
        }
        self.state.status = if changed {
            "Undid edit"
        } else {
            "Nothing to undo"
        }
        .to_string();
        self.refresh();
        Ok(changed)
    }

    pub fn redo(&mut self) -> Result<bool, UiError> {
        let changed = self.api.redo()?;
        if changed {
            self.state.matches.clear();
        }
        self.state.status = if changed {
            "Redid edit"
        } else {
            "Nothing to redo"
        }
        .to_string();
        self.refresh();
        Ok(changed)
    }

    pub fn set_selection(&mut self, selection: Selection) -> Result<(), UiError> {
        self.api.set_selection(selection)?;
        self.state.status = "Selection changed".to_string();
        self.refresh();
        Ok(())
    }

    fn dispatch_text_style(
        &mut self,
        patch: writer_api::core::TextStylePatch,
    ) -> Result<(), UiError> {
        self.api.apply_text_style(patch)?;
        self.state.matches.clear();
        self.state.status = "Text style changed".to_string();
        self.refresh();
        Ok(())
    }

    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.state.mode = mode;
        self.api.set_layout_mode(mode.into());
        self.state.status = "View mode changed".to_string();
        self.refresh();
    }

    /// Changes page geometry and refreshes the layout used by the visual view.
    pub fn set_layout_settings(&mut self, settings: writer_api::layout::LayoutSettings) {
        self.state.mode = settings.mode.into();
        self.state.zoom_percent = settings.zoom_percent;
        self.api.set_layout_settings(settings);
        self.state.status = "Page layout changed".to_string();
        self.refresh();
    }

    pub fn set_zoom_percent(&mut self, percent: u16) -> Result<(), UiError> {
        if !(10..=500).contains(&percent) {
            return Err(UiError::InvalidZoom);
        }
        self.state.zoom_percent = percent;
        self.api.set_zoom_percent(percent);
        self.state.status = format!("Zoom set to {percent}%");
        self.refresh();
        Ok(())
    }

    pub fn find(
        &mut self,
        query: impl Into<writer_api::core::SearchQuery>,
    ) -> Result<usize, UiError> {
        self.state.matches.clear();
        for result in self.api.find(query)? {
            let block_id = match result.container {
                writer_api::core::TextContainerId::Block(id) => id.get(),
                writer_api::core::TextContainerId::Cell(id) => id.get(),
            };
            self.state.matches.push(MatchEntry {
                block_id,
                start: result.range.start().get(),
                end: result.range.end().get(),
            });
        }
        self.state.status = format!("{} matches", self.state.matches.len());
        Ok(self.state.matches.len())
    }

    pub fn replace_all(
        &mut self,
        query: impl Into<SearchQuery>,
        replacement: &str,
    ) -> Result<usize, UiError> {
        let count = self.api.replace_all(query, replacement)?;
        self.state.matches.clear();
        self.state.status = format!("Replaced {count} matches");
        self.refresh();
        Ok(count)
    }

    pub fn export(&self, format: DocumentFormat) -> Result<ExportedDocument, UiError> {
        Ok(self.api.export(format)?)
    }

    pub fn export_docx(&self) -> Result<Vec<u8>, UiError> {
        Ok(self.api.export_docx()?)
    }

    pub fn load_docx(&mut self, bytes: &[u8]) -> Result<(), UiError> {
        self.api.load_docx(bytes)?;
        self.state.matches.clear();
        self.state.status = "DOCX loaded".to_string();
        self.refresh();
        Ok(())
    }

    pub fn render_model(&self) -> RenderModel {
        RenderModel {
            mode: self.state.mode,
            zoom_percent: self.state.zoom_percent,
            pages: self.state.pages.clone(),
            headings: self.state.headings.clone(),
            matches: self.state.matches.clone(),
            status: self.state.status.clone(),
        }
    }

    pub fn toolbar_state(&self) -> ToolbarState {
        let selection = self.api.selection();
        let document = self.api.document();
        self.build_toolbar_state(&selection, &document)
    }

    pub fn toolbar_render_model(&self) -> ToolbarRenderModel {
        let state = self.toolbar_state();
        ToolbarRenderModel {
            groups: vec![
                ToolbarRenderGroup {
                    group: ToolbarGroup::Character,
                    controls: vec![
                        ToolbarControl {
                            group: ToolbarGroup::Character,
                            action: ToolbarActionKind::FontFamily,
                            label: "FontFamily",
                            enabled: state.font_family.enabled,
                            active: false,
                            indeterminate: matches!(state.font_family.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Character,
                            action: ToolbarActionKind::FontSize,
                            label: "FontSize",
                            enabled: state.size_half_points.enabled,
                            active: false,
                            indeterminate: matches!(
                                state.size_half_points.value,
                                ToolbarValue::Mixed
                            ),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Character,
                            action: ToolbarActionKind::Bold,
                            label: "Bold",
                            enabled: state.bold.enabled,
                            active: matches!(state.bold.value, ToolbarValue::Known(true)),
                            indeterminate: matches!(state.bold.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Character,
                            action: ToolbarActionKind::Italic,
                            label: "Italic",
                            enabled: state.italic.enabled,
                            active: matches!(state.italic.value, ToolbarValue::Known(true)),
                            indeterminate: matches!(state.italic.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Character,
                            action: ToolbarActionKind::Underline,
                            label: "Underline",
                            enabled: state.underline.enabled,
                            active: matches!(state.underline.value, ToolbarValue::Known(true)),
                            indeterminate: matches!(state.underline.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Character,
                            action: ToolbarActionKind::Strikethrough,
                            label: "Strikethrough",
                            enabled: state.strikethrough.enabled,
                            active: matches!(state.strikethrough.value, ToolbarValue::Known(true)),
                            indeterminate: matches!(state.strikethrough.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Character,
                            action: ToolbarActionKind::Foreground,
                            label: "Foreground",
                            enabled: state.foreground.enabled,
                            active: false,
                            indeterminate: matches!(state.foreground.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Character,
                            action: ToolbarActionKind::Background,
                            label: "Background",
                            enabled: state.background.enabled,
                            active: false,
                            indeterminate: matches!(state.background.value, ToolbarValue::Mixed),
                        },
                    ],
                },
                ToolbarRenderGroup {
                    group: ToolbarGroup::Paragraph,
                    controls: vec![
                        ToolbarControl {
                            group: ToolbarGroup::Paragraph,
                            action: ToolbarActionKind::AlignLeft,
                            label: "AlignLeft",
                            enabled: state.alignment.enabled,
                            active: matches!(
                                state.alignment.value,
                                ToolbarValue::Known(Alignment::Left)
                            ),
                            indeterminate: matches!(state.alignment.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Paragraph,
                            action: ToolbarActionKind::AlignCenter,
                            label: "AlignCenter",
                            enabled: state.alignment.enabled,
                            active: matches!(
                                state.alignment.value,
                                ToolbarValue::Known(Alignment::Center)
                            ),
                            indeterminate: matches!(state.alignment.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Paragraph,
                            action: ToolbarActionKind::AlignRight,
                            label: "AlignRight",
                            enabled: state.alignment.enabled,
                            active: matches!(
                                state.alignment.value,
                                ToolbarValue::Known(Alignment::Right)
                            ),
                            indeterminate: matches!(state.alignment.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Paragraph,
                            action: ToolbarActionKind::AlignJustified,
                            label: "AlignJustified",
                            enabled: state.alignment.enabled,
                            active: matches!(
                                state.alignment.value,
                                ToolbarValue::Known(Alignment::Justified)
                            ),
                            indeterminate: matches!(state.alignment.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Paragraph,
                            action: ToolbarActionKind::LineSpacing,
                            label: "LineSpacing",
                            enabled: state.line_spacing.enabled,
                            active: false,
                            indeterminate: matches!(state.line_spacing.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Paragraph,
                            action: ToolbarActionKind::Indents,
                            label: "Indents",
                            enabled: state.indent_left_half_points.enabled
                                || state.indent_right_half_points.enabled
                                || state.first_line_indent_half_points.enabled,
                            active: false,
                            indeterminate: false,
                        },
                    ],
                },
                ToolbarRenderGroup {
                    group: ToolbarGroup::Styles,
                    controls: vec![
                        ToolbarControl {
                            group: ToolbarGroup::Styles,
                            action: ToolbarActionKind::ParagraphStyle,
                            label: "Paragraph",
                            enabled: state.block_kind.enabled,
                            active: false,
                            indeterminate: false,
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Styles,
                            action: ToolbarActionKind::Heading1,
                            label: "Heading1",
                            enabled: state.block_kind.enabled,
                            active: matches!(
                                state.block_kind.value,
                                ToolbarValue::Known(BlockKind::Heading(level)) if level.get() == 1
                            ),
                            indeterminate: matches!(state.block_kind.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Styles,
                            action: ToolbarActionKind::Heading2,
                            label: "Heading2",
                            enabled: state.block_kind.enabled,
                            active: matches!(
                                state.block_kind.value,
                                ToolbarValue::Known(BlockKind::Heading(level)) if level.get() == 2
                            ),
                            indeterminate: matches!(state.block_kind.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Styles,
                            action: ToolbarActionKind::Heading3,
                            label: "Heading3",
                            enabled: state.block_kind.enabled,
                            active: matches!(
                                state.block_kind.value,
                                ToolbarValue::Known(BlockKind::Heading(level)) if level.get() == 3
                            ),
                            indeterminate: matches!(state.block_kind.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Styles,
                            action: ToolbarActionKind::Heading4,
                            label: "Heading4",
                            enabled: state.block_kind.enabled,
                            active: matches!(
                                state.block_kind.value,
                                ToolbarValue::Known(BlockKind::Heading(level)) if level.get() == 4
                            ),
                            indeterminate: matches!(state.block_kind.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Styles,
                            action: ToolbarActionKind::Heading5,
                            label: "Heading5",
                            enabled: state.block_kind.enabled,
                            active: matches!(
                                state.block_kind.value,
                                ToolbarValue::Known(BlockKind::Heading(level)) if level.get() == 5
                            ),
                            indeterminate: matches!(state.block_kind.value, ToolbarValue::Mixed),
                        },
                        ToolbarControl {
                            group: ToolbarGroup::Styles,
                            action: ToolbarActionKind::Heading6,
                            label: "Heading6",
                            enabled: state.block_kind.enabled,
                            active: matches!(
                                state.block_kind.value,
                                ToolbarValue::Known(BlockKind::Heading(level)) if level.get() == 6
                            ),
                            indeterminate: matches!(state.block_kind.value, ToolbarValue::Mixed),
                        },
                    ],
                },
            ],
        }
    }

    pub fn dispatch_toolbar(
        &mut self,
        action: ToolbarAction,
    ) -> Result<ToolbarApplyStatus, UiError> {
        let before = self.snapshot();
        let state = self.toolbar_state();

        match action {
            ToolbarAction::SetFontFamily(value) => {
                if !state.font_family.enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                self.dispatch_text_style(TextStylePatch::new().font_family(value))?;
            }
            ToolbarAction::SetSizeHalfPoints(size) => {
                if !(1..=16384).contains(&size) {
                    return Err(UiError::InvalidToolbarSize);
                }
                if !state.size_half_points.enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                self.dispatch_text_style(TextStylePatch::new().size_half_points(size))?;
            }
            ToolbarAction::SetBold(value) => {
                if !state.bold.enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                self.dispatch_text_style(TextStylePatch::new().bold(value))?;
            }
            ToolbarAction::SetItalic(value) => {
                if !state.italic.enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                self.dispatch_text_style(TextStylePatch::new().italic(value))?;
            }
            ToolbarAction::SetUnderline(value) => {
                if !state.underline.enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                self.dispatch_text_style(TextStylePatch::new().underline(value))?;
            }
            ToolbarAction::SetStrikethrough(value) => {
                if !state.strikethrough.enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                self.dispatch_text_style(TextStylePatch::new().strikethrough(value))?;
            }
            ToolbarAction::SetForeground(value) => {
                if !state.foreground.enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                self.dispatch_text_style(TextStylePatch::new().foreground(value))?;
            }
            ToolbarAction::SetBackground(value) => {
                if !state.background.enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                self.dispatch_text_style(TextStylePatch::new().background(value))?;
            }
            ToolbarAction::SetAlignment(alignment) => {
                if !state.alignment.enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                let block_id = self.selected_block_id()?;
                let patch = ParagraphStylePatch::default().alignment(alignment);
                self.execute(Command::SetParagraphStyle { block_id, patch })?;
            }
            ToolbarAction::SetLineSpacing(line_spacing) => {
                if !state.line_spacing.enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                let block_id = self.selected_block_id()?;
                let patch = ParagraphStylePatch::default().line_spacing(line_spacing);
                self.execute(Command::SetParagraphStyle { block_id, patch })?;
            }
            ToolbarAction::SetIndents {
                left_half_points,
                right_half_points,
                first_line_half_points,
            } => {
                let enabled = state.indent_left_half_points.enabled
                    || state.indent_right_half_points.enabled
                    || state.first_line_indent_half_points.enabled;
                if !enabled {
                    return Ok(ToolbarApplyStatus::Disabled);
                }
                let block_id = self.selected_block_id()?;
                let patch = ParagraphStylePatch::default().indents(
                    left_half_points,
                    right_half_points,
                    first_line_half_points,
                );
                self.execute(Command::SetParagraphStyle { block_id, patch })?;
            }
            ToolbarAction::SetHeading(level) => {
                if let Some(level) = level {
                    let kind = BlockKind::Heading(
                        HeadingLevel::new(level).map_err(|_| UiError::InvalidHeadingLevel)?,
                    );
                    let enabled = state.block_kind.enabled;
                    if !enabled {
                        return Ok(ToolbarApplyStatus::Disabled);
                    }
                    let block_id = self.selected_block_id()?;
                    self.execute(Command::SetBlockKind { block_id, kind })?;
                } else {
                    let enabled = state.block_kind.enabled;
                    if !enabled {
                        return Ok(ToolbarApplyStatus::Disabled);
                    }
                    let block_id = self.selected_block_id()?;
                    let kind = BlockKind::Paragraph;
                    self.execute(Command::SetBlockKind { block_id, kind })?;
                }
            }
        }

        let after = self.snapshot();
        if before != after {
            Ok(ToolbarApplyStatus::Applied)
        } else {
            Ok(ToolbarApplyStatus::Unchanged)
        }
    }

    fn refresh(&mut self) {
        let document = self.api.document();
        self.state.headings = document
            .blocks
            .iter()
            .filter_map(|block| {
                let text = block.text_block()?;
                let BlockKind::Heading(level) = text.kind else {
                    return None;
                };
                Some(HeadingEntry {
                    block_id: text.id.get(),
                    level: level.get(),
                    text: text.text.clone(),
                })
            })
            .collect();
        self.state.selected_block = self.selected_block();
        match self.api.layout() {
            Ok(layout) => {
                self.state.pages = layout
                    .pages
                    .iter()
                    .map(|page| PageEntry {
                        index: page.index,
                        block_count: page.blocks.len(),
                        height_half_points: page.height_half_points,
                    })
                    .collect();
                self.state.layout = Some(layout);
            }
            Err(_) => {
                self.state.pages.clear();
                self.state.layout = None;
            }
        }
    }

    fn selected_block(&self) -> Option<u64> {
        self.api
            .selection()
            .range()
            .and_then(|(container, _)| match container {
                TextContainerId::Block(id) => Some(id.get()),
                TextContainerId::Cell(_) => None,
            })
    }

    fn selected_block_id(&self) -> Result<writer_api::core::BlockId, UiError> {
        self.selected_block()
            .map(writer_api::core::BlockId::new)
            .ok_or(UiError::InvalidSelection)
    }

    fn build_toolbar_state(
        &self,
        selection: &Selection,
        document: &writer_api::core::DocumentSnapshot,
    ) -> ToolbarState {
        let (char_state, block_state) = match selection.range() {
            Some((TextContainerId::Block(block_id), range)) => {
                let block = document
                    .blocks
                    .iter()
                    .find(|block| block.id() == block_id.get());
                let block_state = block.and_then(|block| self.block_paragraph_state(block));
                let char_state = block
                    .and_then(|block| block.text_block())
                    .map(|text_block| self.char_state(text_block, range));
                (char_state, block_state)
            }
            Some((TextContainerId::Cell(cell_id), range)) => {
                let char_state = document.blocks.iter().find_map(|block| match block {
                    writer_api::core::BlockSnapshot::Table(table) => table
                        .rows
                        .iter()
                        .flat_map(|row| row.cells.iter())
                        .find(|cell| cell.id.get() == cell_id.get())
                        .map(|cell| self.cell_char_state(cell, range)),
                    _ => None,
                });
                (char_state, None)
            }
            None => (None, None),
        };

        let collapsed = selection.is_collapsed();
        if collapsed {
            return ToolbarState {
                font_family: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                size_half_points: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                bold: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                italic: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                underline: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                strikethrough: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                foreground: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                background: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                alignment: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.alignment))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                line_spacing: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.line_spacing))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                indent_left_half_points: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.indent_left_half_points))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                indent_right_half_points: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.indent_right_half_points))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                first_line_indent_half_points: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.first_line_indent_half_points))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                block_kind: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.kind.clone()))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                lists_enabled: false,
            };
        }

        match char_state {
            Some(char_state) => ToolbarState {
                font_family: ToolbarField {
                    value: char_state.font_family,
                    enabled: true,
                },
                size_half_points: ToolbarField {
                    value: char_state.size,
                    enabled: true,
                },
                bold: ToolbarField {
                    value: char_state.bold,
                    enabled: true,
                },
                italic: ToolbarField {
                    value: char_state.italic,
                    enabled: true,
                },
                underline: ToolbarField {
                    value: char_state.underline,
                    enabled: true,
                },
                strikethrough: ToolbarField {
                    value: char_state.strikethrough,
                    enabled: true,
                },
                foreground: ToolbarField {
                    value: char_state.foreground,
                    enabled: true,
                },
                background: ToolbarField {
                    value: char_state.background,
                    enabled: true,
                },
                alignment: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.alignment))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                line_spacing: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.line_spacing))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                indent_left_half_points: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.indent_left_half_points))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                indent_right_half_points: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.indent_right_half_points))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                first_line_indent_half_points: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.first_line_indent_half_points))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                block_kind: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.kind.clone()))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                lists_enabled: false,
            },
            None => ToolbarState {
                font_family: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                size_half_points: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                bold: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                italic: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                underline: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                strikethrough: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                foreground: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                background: ToolbarField {
                    value: ToolbarValue::Unavailable,
                    enabled: false,
                },
                alignment: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.alignment))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                line_spacing: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.line_spacing))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                indent_left_half_points: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.indent_left_half_points))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                indent_right_half_points: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.indent_right_half_points))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                first_line_indent_half_points: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.first_line_indent_half_points))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                block_kind: ToolbarField {
                    value: block_state
                        .as_ref()
                        .map(|state| ToolbarValue::Known(state.kind.clone()))
                        .unwrap_or(ToolbarValue::Unavailable),
                    enabled: block_state.is_some(),
                },
                lists_enabled: false,
            },
        }
    }

    fn char_state(
        &self,
        text_block: &writer_api::core::TextBlockSnapshot,
        range: writer_api::core::TextRange,
    ) -> CharAggregate {
        let mut aggregate = CharAggregate::default();
        if range.start().get() == range.end().get() {
            return aggregate;
        }

        let mut cursor = 0usize;
        for run in &text_block.runs {
            let run_start = cursor;
            let run_end = cursor + run.text.chars().count();
            cursor = run_end;

            let overlap_start = run_start.max(range.start().get());
            let overlap_end = run_end.min(range.end().get());
            if overlap_start >= overlap_end {
                continue;
            }

            let font_family = run.style.font_family().map(|value| value.to_string());
            let current = aggregate.font_family.clone();
            aggregate.font_family = aggregate.merge_option(current, font_family);
            aggregate.size = aggregate.merge(aggregate.size, run.style.size_half_points());
            aggregate.bold = aggregate.merge(aggregate.bold, run.style.bold());
            aggregate.italic = aggregate.merge(aggregate.italic, run.style.italic());
            aggregate.underline = aggregate.merge(aggregate.underline, run.style.underline());
            aggregate.strikethrough =
                aggregate.merge(aggregate.strikethrough, run.style.strikethrough());
            aggregate.foreground =
                aggregate.merge_option(aggregate.foreground, run.style.foreground());
            aggregate.background =
                aggregate.merge_option(aggregate.background, run.style.background());
        }

        aggregate
    }

    fn cell_char_state(
        &self,
        cell: &writer_api::core::TableCellSnapshot,
        range: writer_api::core::TextRange,
    ) -> CharAggregate {
        let mut aggregate = CharAggregate::default();
        if range.start().get() == range.end().get() {
            return aggregate;
        }

        let mut cursor = 0usize;
        for run in &cell.runs {
            let run_start = cursor;
            let run_end = cursor + run.text.chars().count();
            cursor = run_end;

            let overlap_start = run_start.max(range.start().get());
            let overlap_end = run_end.min(range.end().get());
            if overlap_start >= overlap_end {
                continue;
            }

            let font_family = run.style.font_family().map(|value| value.to_string());
            let current = aggregate.font_family.clone();
            aggregate.font_family = aggregate.merge_option(current, font_family);
            aggregate.size = aggregate.merge(aggregate.size, run.style.size_half_points());
            aggregate.bold = aggregate.merge(aggregate.bold, run.style.bold());
            aggregate.italic = aggregate.merge(aggregate.italic, run.style.italic());
            aggregate.underline = aggregate.merge(aggregate.underline, run.style.underline());
            aggregate.strikethrough =
                aggregate.merge(aggregate.strikethrough, run.style.strikethrough());
            aggregate.foreground =
                aggregate.merge_option(aggregate.foreground, run.style.foreground());
            aggregate.background =
                aggregate.merge_option(aggregate.background, run.style.background());
        }

        aggregate
    }

    fn block_paragraph_state(
        &self,
        block: &writer_api::core::BlockSnapshot,
    ) -> Option<BlockParagraphState> {
        block.text_block().map(|text_block| BlockParagraphState {
            kind: text_block.kind.clone(),
            alignment: text_block.paragraph_style.alignment(),
            line_spacing: text_block.paragraph_style.line_spacing(),
            indent_left_half_points: text_block.paragraph_style.indent_left_half_points(),
            indent_right_half_points: text_block.paragraph_style.indent_right_half_points(),
            first_line_indent_half_points: text_block
                .paragraph_style
                .first_line_indent_half_points(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct CharAggregate {
    font_family: ToolbarValue<Option<String>>,
    size: ToolbarValue<u16>,
    bold: ToolbarValue<bool>,
    italic: ToolbarValue<bool>,
    underline: ToolbarValue<bool>,
    strikethrough: ToolbarValue<bool>,
    foreground: ToolbarValue<Option<Rgba8>>,
    background: ToolbarValue<Option<Rgba8>>,
}

impl CharAggregate {
    fn merge<T: PartialEq + Copy>(&self, current: ToolbarValue<T>, next: T) -> ToolbarValue<T> {
        match current {
            ToolbarValue::Known(current_value) => {
                if current_value == next {
                    ToolbarValue::Known(current_value)
                } else {
                    ToolbarValue::Mixed
                }
            }
            ToolbarValue::Mixed => ToolbarValue::Mixed,
            ToolbarValue::Unavailable => ToolbarValue::Known(next),
        }
    }

    fn merge_option<T: PartialEq + Clone>(
        &self,
        current: ToolbarValue<Option<T>>,
        next: Option<T>,
    ) -> ToolbarValue<Option<T>> {
        match current {
            ToolbarValue::Known(current_value) => {
                if current_value == next {
                    ToolbarValue::Known(current_value)
                } else {
                    ToolbarValue::Mixed
                }
            }
            ToolbarValue::Mixed => ToolbarValue::Mixed,
            ToolbarValue::Unavailable => ToolbarValue::Known(next),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct BlockParagraphState {
    kind: BlockKind,
    alignment: Alignment,
    line_spacing: LineSpacing,
    indent_left_half_points: i32,
    indent_right_half_points: i32,
    first_line_indent_half_points: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderModel {
    pub mode: ViewMode,
    pub zoom_percent: u16,
    pub pages: Vec<PageEntry>,
    pub headings: Vec<HeadingEntry>,
    pub matches: Vec<MatchEntry>,
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use writer_api::core::{BlockId, Command, Cursor, Selection, TextContainerId, TextOffset};

    #[test]
    fn toolbar_normal_order_reflects_character_paragraph_and_styles() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("hello").unwrap();

        let selection = Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(5)),
        );
        ui.set_selection(selection).unwrap();
        let state = ui.toolbar_state();
        let render = ui.toolbar_render_model();

        assert!(state.font_family.enabled);
        assert!(matches!(state.font_family.value, ToolbarValue::Known(_)));
        assert!(state.alignment.enabled);
        assert!(matches!(
            state.alignment.value,
            ToolbarValue::Known(Alignment::Left)
        ));
        assert_eq!(render.groups.len(), 3);
        assert_eq!(render.groups[0].group, ToolbarGroup::Character);
        assert_eq!(render.groups[1].group, ToolbarGroup::Paragraph);
        assert_eq!(render.groups[2].group, ToolbarGroup::Styles);
        assert!(
            !render
                .groups
                .iter()
                .any(|group| group.group == ToolbarGroup::Character && group.controls.is_empty())
        );
    }

    #[test]
    fn toolbar_mixed_bold_then_setter_applies_and_clears_mixed() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("bold").unwrap();

        let partial = Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(2)),
        );
        ui.set_selection(partial).unwrap();
        ui.dispatch_toolbar(ToolbarAction::SetBold(true)).unwrap();

        let full = Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(4)),
        );
        ui.set_selection(full).unwrap();

        let state = ui.toolbar_state();
        assert!(matches!(state.bold.value, ToolbarValue::Mixed));
        assert_eq!(
            ui.dispatch_toolbar(ToolbarAction::SetBold(true)).unwrap(),
            ToolbarApplyStatus::Applied
        );

        let state = ui.toolbar_state();
        assert!(matches!(state.bold.value, ToolbarValue::Known(true)));
    }

    #[test]
    fn toolbar_collapsed_block_shows_paragraph_and_heading_controls() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("heading").unwrap();

        let selection = Selection::collapsed(Cursor::new(
            TextContainerId::Block(BlockId::new(1)),
            TextOffset::new(0),
        ));
        ui.set_selection(selection).unwrap();
        let state = ui.toolbar_state();
        assert!(!state.font_family.enabled);
        assert!(!matches!(state.font_family.value, ToolbarValue::Known(_)));
        assert!(state.alignment.enabled);
        assert!(matches!(
            state.alignment.value,
            ToolbarValue::Known(Alignment::Left)
        ));
        assert!(state.block_kind.enabled);
        assert!(matches!(
            state.block_kind.value,
            ToolbarValue::Known(BlockKind::Paragraph)
        ));
    }

    #[test]
    fn toolbar_image_selection_disables_character_and_paragraph_controls() {
        let mut ui = WriterViewModel::new();
        ui.api_mut()
            .execute(Command::InsertImage {
                payload: writer_api::core::ImagePayload::new(
                    writer_api::core::ImageFormat::Png,
                    writer_api::core::PixelSize::new(1, 1).unwrap(),
                    vec![0x89, 0x50, 0x4e, 0x47],
                )
                .unwrap(),
            })
            .unwrap();

        let state = ui.toolbar_state();
        assert!(!state.font_family.enabled);
        assert!(!state.bold.enabled);
        assert!(!state.alignment.enabled);
        assert!(!state.block_kind.enabled);
    }

    #[test]
    fn toolbar_repeat_same_state_is_unchanged() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("hello").unwrap();

        let selection = Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(5)),
        );
        ui.set_selection(selection).unwrap();

        let first = ui.dispatch_toolbar(ToolbarAction::SetBold(true));
        assert_eq!(first.unwrap(), ToolbarApplyStatus::Applied);

        let second = ui.dispatch_toolbar(ToolbarAction::SetBold(true));
        assert_eq!(second.unwrap(), ToolbarApplyStatus::Unchanged);
    }

    #[test]
    fn toolbar_invalid_size_returns_error_without_mutation() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("hello").unwrap();

        let selection = Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(5)),
        );
        ui.set_selection(selection).unwrap();

        let before = ui.snapshot();
        let status = ui.dispatch_toolbar(ToolbarAction::SetSizeHalfPoints(0));
        assert_eq!(status, Err(UiError::InvalidToolbarSize));
        assert_eq!(ui.snapshot(), before);

        let collapsed = Selection::collapsed(Cursor::new(
            TextContainerId::Block(BlockId::new(1)),
            TextOffset::new(0),
        ));
        ui.set_selection(collapsed).unwrap();
        let before_collapsed = ui.snapshot();
        let status_collapsed = ui.dispatch_toolbar(ToolbarAction::SetSizeHalfPoints(16384 + 1));
        assert_eq!(status_collapsed, Err(UiError::InvalidToolbarSize));
        assert_eq!(ui.snapshot(), before_collapsed);
    }

    #[test]
    fn toolbar_alignment_update_changes_paragraph_style() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("hello").unwrap();

        let selection = Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(5)),
        );
        ui.set_selection(selection).unwrap();

        let status = ui.dispatch_toolbar(ToolbarAction::SetAlignment(Alignment::Center));
        assert_eq!(status.unwrap(), ToolbarApplyStatus::Applied);

        let state = ui.toolbar_state();
        assert!(matches!(
            state.alignment.value,
            ToolbarValue::Known(Alignment::Center)
        ));
    }

    #[test]
    fn toolbar_text_style_after_find_clears_matches_and_refreshes_status() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("plain only").unwrap();

        let selection = Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(9)),
        );
        ui.set_selection(selection).unwrap();
        ui.find("plain").unwrap();

        assert_eq!(
            ui.dispatch_toolbar(ToolbarAction::SetBold(true)).unwrap(),
            ToolbarApplyStatus::Applied
        );
        assert!(ui.state().matches.is_empty());
        assert_eq!(ui.render_model().status, "Text style changed");
        assert_eq!(ui.state().selected_block, Some(1));
        assert!(!ui.state().pages.is_empty());
    }

    #[test]
    fn toolbar_invalid_heading_levels_return_error_without_mutation() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("select me").unwrap();

        let selection = Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(8)),
        );
        ui.set_selection(selection).unwrap();

        let before = ui.state().clone();
        let snapshot = ui.snapshot();
        assert_eq!(
            ui.dispatch_toolbar(ToolbarAction::SetHeading(Some(0))),
            Err(UiError::InvalidHeadingLevel)
        );
        assert_eq!(
            ui.dispatch_toolbar(ToolbarAction::SetHeading(Some(7))),
            Err(UiError::InvalidHeadingLevel)
        );
        assert_eq!(ui.state(), &before);
        assert_eq!(ui.snapshot(), snapshot);

        let collapsed = Selection::collapsed(Cursor::new(
            TextContainerId::Block(BlockId::new(1)),
            TextOffset::new(0),
        ));
        ui.set_selection(collapsed).unwrap();
        let before_collapsed = ui.state().clone();
        let snapshot_collapsed = ui.snapshot();
        assert_eq!(
            ui.dispatch_toolbar(ToolbarAction::SetHeading(Some(0))),
            Err(UiError::InvalidHeadingLevel)
        );
        assert_eq!(
            ui.dispatch_toolbar(ToolbarAction::SetHeading(Some(7))),
            Err(UiError::InvalidHeadingLevel)
        );
        assert_eq!(ui.state(), &before_collapsed);
        assert_eq!(ui.snapshot(), snapshot_collapsed);
    }

    #[test]
    fn toolbar_collapsed_selection_set_bold_returns_disabled_without_mutation() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("keep this").unwrap();

        let selection = Selection::collapsed(Cursor::new(
            TextContainerId::Block(BlockId::new(1)),
            TextOffset::new(0),
        ));
        ui.set_selection(selection).unwrap();

        let before = ui.state().clone();
        let snapshot = ui.snapshot();
        assert_eq!(
            ui.dispatch_toolbar(ToolbarAction::SetBold(true)),
            Ok(ToolbarApplyStatus::Disabled)
        );
        assert_eq!(ui.state(), &before);
        assert_eq!(ui.snapshot(), snapshot);
    }

    #[test]
    fn toolbar_table_cell_keeps_character_formatting_and_disables_paragraph_controls() {
        let mut ui = WriterViewModel::new();
        ui.execute(Command::InsertTable {
            rows: 1,
            columns: 1,
        })
        .unwrap();
        ui.insert_text("cell").unwrap();

        ui.set_selection(Selection::new(
            Cursor::new(
                TextContainerId::Cell(writer_api::core::CellId::new(1)),
                TextOffset::new(0),
            ),
            Cursor::new(
                TextContainerId::Cell(writer_api::core::CellId::new(1)),
                TextOffset::new(4),
            ),
        ))
        .unwrap();

        let state = ui.toolbar_state();
        assert!(state.bold.enabled);
        assert_eq!(state.bold.value, ToolbarValue::Known(false));
        assert!(state.italic.enabled);
        assert_eq!(state.italic.value, ToolbarValue::Known(false));
        assert!(state.font_family.enabled);
        assert_eq!(state.font_family.value, ToolbarValue::Known(None));
        assert_eq!(state.alignment.value, ToolbarValue::Unavailable);
        assert!(!state.alignment.enabled);
        assert_eq!(state.line_spacing.value, ToolbarValue::Unavailable);
        assert!(!state.line_spacing.enabled);
        assert_eq!(
            state.indent_left_half_points.value,
            ToolbarValue::Unavailable
        );
        assert!(!state.indent_left_half_points.enabled);
        assert_eq!(
            state.indent_right_half_points.value,
            ToolbarValue::Unavailable
        );
        assert!(!state.indent_right_half_points.enabled);
        assert_eq!(
            state.first_line_indent_half_points.value,
            ToolbarValue::Unavailable
        );
        assert!(!state.first_line_indent_half_points.enabled);
        assert_eq!(state.block_kind.value, ToolbarValue::Unavailable);
        assert!(!state.block_kind.enabled);

        let before_state = ui.state().clone();
        let before_snapshot = ui.snapshot();
        assert_eq!(
            ui.dispatch_toolbar(ToolbarAction::SetAlignment(Alignment::Center)),
            Ok(ToolbarApplyStatus::Disabled)
        );
        assert_eq!(ui.state(), &before_state);
        assert_eq!(ui.snapshot(), before_snapshot);
    }

    #[test]
    fn toolbar_undo_redo_italic_round_trip_restores_toolbar_state() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("hello").unwrap();

        let selection = Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(5)),
        );
        ui.set_selection(selection).unwrap();

        let before = ui.snapshot();
        assert_eq!(
            ui.dispatch_toolbar(ToolbarAction::SetItalic(true)).unwrap(),
            ToolbarApplyStatus::Applied
        );
        let styled = ui.snapshot();
        assert!(ui.undo().unwrap());
        assert_eq!(ui.snapshot().blocks, before.blocks);
        assert!(ui.redo().unwrap());
        assert_eq!(ui.snapshot().blocks, styled.blocks);
        assert!(matches!(
            ui.toolbar_state().italic.value,
            ToolbarValue::Known(true)
        ));
    }

    #[test]
    fn toolbar_state_and_render_model_are_deterministic_and_ordered() {
        let mut ui = WriterViewModel::new();
        ui.insert_text("stable normal text").unwrap();
        ui.set_selection(Selection::new(
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(0)),
            Cursor::new(TextContainerId::Block(BlockId::new(1)), TextOffset::new(18)),
        ))
        .unwrap();

        let first_state = ui.toolbar_state();
        for _ in 0..4 {
            assert_eq!(ui.toolbar_state(), first_state);
        }
        let first_render = ui.toolbar_render_model();
        for _ in 0..4 {
            assert_eq!(ui.toolbar_render_model(), first_render);
        }

        let actions = first_render
            .groups
            .iter()
            .flat_map(|group| group.controls.iter().map(|control| control.action));
        assert_eq!(
            actions.collect::<Vec<_>>(),
            vec![
                ToolbarActionKind::FontFamily,
                ToolbarActionKind::FontSize,
                ToolbarActionKind::Bold,
                ToolbarActionKind::Italic,
                ToolbarActionKind::Underline,
                ToolbarActionKind::Strikethrough,
                ToolbarActionKind::Foreground,
                ToolbarActionKind::Background,
                ToolbarActionKind::AlignLeft,
                ToolbarActionKind::AlignCenter,
                ToolbarActionKind::AlignRight,
                ToolbarActionKind::AlignJustified,
                ToolbarActionKind::LineSpacing,
                ToolbarActionKind::Indents,
                ToolbarActionKind::ParagraphStyle,
                ToolbarActionKind::Heading1,
                ToolbarActionKind::Heading2,
                ToolbarActionKind::Heading3,
                ToolbarActionKind::Heading4,
                ToolbarActionKind::Heading5,
                ToolbarActionKind::Heading6,
            ]
        );
    }
}
