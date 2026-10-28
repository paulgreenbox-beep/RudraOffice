#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

id_type!(BlockId);
id_type!(TableId);
id_type!(RowId);
id_type!(ColumnId);
id_type!(CellId);
id_type!(ImageId);
id_type!(ListId);
id_type!(Revision);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }
    pub const fn red(self) -> u8 {
        self.r
    }
    pub const fn green(self) -> u8 {
        self.g
    }
    pub const fn blue(self) -> u8 {
        self.b
    }
    pub const fn alpha(self) -> u8 {
        self.a
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextStyle {
    font_family: Option<String>,
    size_half_points: u16,
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    foreground: Option<Rgba8>,
    background: Option<Rgba8>,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: None,
            size_half_points: 22,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            foreground: None,
            background: None,
        }
    }
}

impl TextStyle {
    pub fn new(size_half_points: u16) -> Result<Self, ModelError> {
        validate_half_points(size_half_points)?;
        Ok(Self {
            size_half_points,
            ..Self::default()
        })
    }
    pub fn font_family(&self) -> Option<&str> {
        self.font_family.as_deref()
    }
    pub fn size_half_points(&self) -> u16 {
        self.size_half_points
    }
    pub fn bold(&self) -> bool {
        self.bold
    }
    pub fn italic(&self) -> bool {
        self.italic
    }
    pub fn underline(&self) -> bool {
        self.underline
    }
    pub fn strikethrough(&self) -> bool {
        self.strikethrough
    }
    pub fn foreground(&self) -> Option<Rgba8> {
        self.foreground
    }
    pub fn background(&self) -> Option<Rgba8> {
        self.background
    }
    pub fn with_font_family(mut self, value: Option<String>) -> Self {
        self.font_family = value;
        self
    }
    pub fn with_bold(mut self, value: bool) -> Self {
        self.bold = value;
        self
    }
    pub fn with_italic(mut self, value: bool) -> Self {
        self.italic = value;
        self
    }
    pub fn with_underline(mut self, value: bool) -> Self {
        self.underline = value;
        self
    }
    pub fn with_strikethrough(mut self, value: bool) -> Self {
        self.strikethrough = value;
        self
    }
    pub fn with_foreground(mut self, value: Option<Rgba8>) -> Self {
        self.foreground = value;
        self
    }
    pub fn with_background(mut self, value: Option<Rgba8>) -> Self {
        self.background = value;
        self
    }
    pub fn with_size_half_points(mut self, value: u16) -> Result<Self, ModelError> {
        validate_half_points(value)?;
        self.size_half_points = value;
        Ok(self)
    }
    fn apply_patch(&mut self, patch: &TextStylePatch) -> Result<(), ModelError> {
        if let Some(value) = patch.font_family.as_ref() {
            self.font_family = value.clone();
        }
        if let Some(value) = patch.size_half_points {
            validate_half_points(value)?;
            self.size_half_points = value;
        }
        if let Some(value) = patch.bold {
            self.bold = value;
        }
        if let Some(value) = patch.italic {
            self.italic = value;
        }
        if let Some(value) = patch.underline {
            self.underline = value;
        }
        if let Some(value) = patch.strikethrough {
            self.strikethrough = value;
        }
        if let Some(value) = patch.foreground {
            self.foreground = value;
        }
        if let Some(value) = patch.background {
            self.background = value;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextStylePatch {
    /// `Some(Some(value))` sets a value; `Some(None)` clears it.
    pub font_family: Option<Option<String>>,
    pub size_half_points: Option<u16>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub foreground: Option<Option<Rgba8>>,
    pub background: Option<Option<Rgba8>>,
}

impl TextStylePatch {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn font_family(mut self, value: Option<String>) -> Self {
        self.font_family = Some(value);
        self
    }
    pub fn size_half_points(mut self, value: u16) -> Self {
        self.size_half_points = Some(value);
        self
    }
    pub fn bold(mut self, value: bool) -> Self {
        self.bold = Some(value);
        self
    }
    pub fn italic(mut self, value: bool) -> Self {
        self.italic = Some(value);
        self
    }
    pub fn underline(mut self, value: bool) -> Self {
        self.underline = Some(value);
        self
    }
    pub fn strikethrough(mut self, value: bool) -> Self {
        self.strikethrough = Some(value);
        self
    }
    pub fn foreground(mut self, value: Option<Rgba8>) -> Self {
        self.foreground = Some(value);
        self
    }
    pub fn background(mut self, value: Option<Rgba8>) -> Self {
        self.background = Some(value);
        self
    }
    pub fn apply(&self, style: &TextStyle) -> Result<TextStyle, ModelError> {
        let mut result = style.clone();
        result.apply_patch(self)?;
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justified,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineSpacing {
    #[default]
    Single,
    OneAndHalf,
    Double,
    Multiple(u16),
}

impl LineSpacing {
    pub fn multiple(value: u16) -> Result<Self, ModelError> {
        if value == 0 {
            return Err(ModelError::InvalidValue(
                "line spacing multiplier must be non-zero",
            ));
        }
        Ok(Self::Multiple(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HeadingLevel(u8);

impl HeadingLevel {
    pub fn new(value: u8) -> Result<Self, ModelError> {
        if (1..=6).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ModelError::InvalidValue(
                "heading level must be between 1 and 6",
            ))
        }
    }
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Numbered,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum BlockKind {
    #[default]
    Paragraph,
    Heading(HeadingLevel),
    ListItem {
        list_id: ListId,
        kind: ListKind,
    },
    /// An explicit document page boundary imported from or exported to DOCX.
    PageBreak,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParagraphStyle {
    alignment: Alignment,
    line_spacing: LineSpacing,
    indent_left_half_points: i32,
    indent_right_half_points: i32,
    first_line_indent_half_points: i32,
    space_before_half_points: u32,
    space_after_half_points: u32,
}

impl Default for ParagraphStyle {
    fn default() -> Self {
        Self {
            alignment: Alignment::Left,
            line_spacing: LineSpacing::Single,
            indent_left_half_points: 0,
            indent_right_half_points: 0,
            first_line_indent_half_points: 0,
            space_before_half_points: 0,
            space_after_half_points: 0,
        }
    }
}

impl ParagraphStyle {
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }
    pub fn line_spacing(&self) -> LineSpacing {
        self.line_spacing
    }
    pub fn indent_left_half_points(&self) -> i32 {
        self.indent_left_half_points
    }
    pub fn indent_right_half_points(&self) -> i32 {
        self.indent_right_half_points
    }
    pub fn first_line_indent_half_points(&self) -> i32 {
        self.first_line_indent_half_points
    }
    pub fn space_before_half_points(&self) -> u32 {
        self.space_before_half_points
    }
    pub fn space_after_half_points(&self) -> u32 {
        self.space_after_half_points
    }
    pub fn with_alignment(mut self, value: Alignment) -> Self {
        self.alignment = value;
        self
    }
    pub fn with_line_spacing(mut self, value: LineSpacing) -> Self {
        self.line_spacing = value;
        self
    }
    pub fn with_indents(mut self, left: i32, right: i32, first_line: i32) -> Self {
        self.indent_left_half_points = left;
        self.indent_right_half_points = right;
        self.first_line_indent_half_points = first_line;
        self
    }
    pub fn with_spacing(mut self, before: u32, after: u32) -> Self {
        self.space_before_half_points = before;
        self.space_after_half_points = after;
        self
    }
    fn apply_patch(&mut self, patch: &ParagraphStylePatch) {
        if let Some(value) = patch.alignment {
            self.alignment = value;
        }
        if let Some(value) = patch.line_spacing {
            self.line_spacing = value;
        }
        if let Some(value) = patch.indent_left_half_points {
            self.indent_left_half_points = value;
        }
        if let Some(value) = patch.indent_right_half_points {
            self.indent_right_half_points = value;
        }
        if let Some(value) = patch.first_line_indent_half_points {
            self.first_line_indent_half_points = value;
        }
        if let Some(value) = patch.space_before_half_points {
            self.space_before_half_points = value;
        }
        if let Some(value) = patch.space_after_half_points {
            self.space_after_half_points = value;
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParagraphStylePatch {
    pub alignment: Option<Alignment>,
    pub line_spacing: Option<LineSpacing>,
    pub indent_left_half_points: Option<i32>,
    pub indent_right_half_points: Option<i32>,
    pub first_line_indent_half_points: Option<i32>,
    pub space_before_half_points: Option<u32>,
    pub space_after_half_points: Option<u32>,
}

impl ParagraphStylePatch {
    pub fn alignment(mut self, value: Alignment) -> Self {
        self.alignment = Some(value);
        self
    }
    pub fn line_spacing(mut self, value: LineSpacing) -> Self {
        self.line_spacing = Some(value);
        self
    }
    pub fn indents(mut self, left: i32, right: i32, first_line: i32) -> Self {
        self.indent_left_half_points = Some(left);
        self.indent_right_half_points = Some(right);
        self.first_line_indent_half_points = Some(first_line);
        self
    }
    pub fn spacing(mut self, before: u32, after: u32) -> Self {
        self.space_before_half_points = Some(before);
        self.space_after_half_points = Some(after);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextOffset(usize);
impl TextOffset {
    pub const fn new(value: usize) -> Self {
        Self(value)
    }
    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextContainerId {
    Block(BlockId),
    Cell(CellId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cursor {
    container: TextContainerId,
    offset: TextOffset,
}
impl Cursor {
    pub const fn new(container: TextContainerId, offset: TextOffset) -> Self {
        Self { container, offset }
    }
    pub const fn container(self) -> TextContainerId {
        self.container
    }
    pub const fn offset(self) -> TextOffset {
        self.offset
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextRange {
    start: TextOffset,
    end: TextOffset,
}
impl TextRange {
    pub const fn new(start: TextOffset, end: TextOffset) -> Self {
        Self { start, end }
    }
    pub const fn start(self) -> TextOffset {
        self.start
    }
    pub const fn end(self) -> TextOffset {
        self.end
    }
    pub const fn is_collapsed(self) -> bool {
        self.start.get() == self.end.get()
    }
    pub fn ordered(self) -> Self {
        if self.start.get() <= self.end.get() {
            self
        } else {
            Self::new(self.end, self.start)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Selection {
    anchor: Cursor,
    active: Cursor,
}
impl Selection {
    pub const fn new(anchor: Cursor, active: Cursor) -> Self {
        Self { anchor, active }
    }
    pub const fn collapsed(cursor: Cursor) -> Self {
        Self::new(cursor, cursor)
    }
    pub const fn anchor(self) -> Cursor {
        self.anchor
    }
    pub const fn active(self) -> Cursor {
        self.active
    }
    pub fn is_collapsed(self) -> bool {
        self.anchor == self.active
    }
    pub fn range(self) -> Option<(TextContainerId, TextRange)> {
        if self.anchor.container != self.active.container {
            return None;
        }
        Some((
            self.anchor.container,
            TextRange::new(self.anchor.offset, self.active.offset).ordered(),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineRun {
    pub text: String,
    pub style: TextStyle,
}
impl InlineRun {
    pub fn new(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextBlockSnapshot {
    pub id: BlockId,
    pub kind: BlockKind,
    pub paragraph_style: ParagraphStyle,
    pub text: String,
    pub runs: Vec<InlineRun>,
}
impl TextBlockSnapshot {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn len(&self) -> usize {
        self.text.chars().count()
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderStyle {
    None,
    Solid,
    Dashed,
    Dotted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Border {
    pub style: BorderStyle,
    pub width_half_points: u16,
    pub color: Rgba8,
}
impl Default for Border {
    fn default() -> Self {
        Self {
            style: BorderStyle::None,
            width_half_points: 0,
            color: Rgba8::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Borders {
    pub top: Border,
    pub right: Border,
    pub bottom: Border,
    pub left: Border,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellStyle {
    pub background: Option<Rgba8>,
    pub borders: Borders,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellAlignment {
    #[default]
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCellSnapshot {
    pub id: CellId,
    pub text: String,
    pub runs: Vec<InlineRun>,
    pub style: CellStyle,
    pub alignment: CellAlignment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRowSnapshot {
    pub id: RowId,
    pub cells: Vec<TableCellSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableColumnSnapshot {
    pub id: ColumnId,
    pub width_half_points: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSnapshot {
    pub id: TableId,
    pub columns: Vec<TableColumnSnapshot>,
    pub rows: Vec<TableRowSnapshot>,
}
impl TableSnapshot {
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    WebP,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PixelSize {
    pub width: u32,
    pub height: u32,
}
impl PixelSize {
    pub fn new(width: u32, height: u32) -> Result<Self, ModelError> {
        if width == 0 || height == 0 {
            return Err(ModelError::InvalidValue(
                "image dimensions must be non-zero",
            ));
        }
        Ok(Self { width, height })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImagePayload {
    pub format: ImageFormat,
    pub dimensions: PixelSize,
    pub bytes: Vec<u8>,
}
impl ImagePayload {
    pub fn new(
        format: ImageFormat,
        dimensions: PixelSize,
        bytes: Vec<u8>,
    ) -> Result<Self, ModelError> {
        if bytes.is_empty() {
            return Err(ModelError::InvalidValue("image payload must not be empty"));
        }
        if dimensions.width == 0 || dimensions.height == 0 {
            return Err(ModelError::InvalidValue(
                "image dimensions must be non-zero",
            ));
        }
        Ok(Self {
            format,
            dimensions,
            bytes,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLayout {
    pub size: PixelSize,
    pub alignment: Alignment,
    pub aspect_locked: bool,
}
impl Default for ImageLayout {
    fn default() -> Self {
        Self {
            size: PixelSize {
                width: 1,
                height: 1,
            },
            alignment: Alignment::Left,
            aspect_locked: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageSnapshot {
    pub id: ImageId,
    pub payload: ImagePayload,
    pub layout: ImageLayout,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockSnapshot {
    Text(TextBlockSnapshot),
    Table(TableSnapshot),
    Image(ImageSnapshot),
}
impl BlockSnapshot {
    pub fn id(&self) -> u64 {
        match self {
            Self::Text(value) => value.id.get(),
            Self::Table(value) => value.id.get(),
            Self::Image(value) => value.id.get(),
        }
    }
    pub fn text(&self) -> Option<&str> {
        if let Self::Text(value) = self {
            Some(&value.text)
        } else {
            None
        }
    }
    pub fn text_block(&self) -> Option<&TextBlockSnapshot> {
        if let Self::Text(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn table(&self) -> Option<&TableSnapshot> {
        if let Self::Table(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn image(&self) -> Option<&ImageSnapshot> {
        if let Self::Image(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentSnapshot {
    pub blocks: Vec<BlockSnapshot>,
    pub revision: Revision,
}
impl DocumentSnapshot {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.blocks.is_empty() {
            return Err(ModelError::InvalidDocument("document must contain a block"));
        }
        let mut text_ids = std::collections::BTreeSet::new();
        let mut table_ids = std::collections::BTreeSet::new();
        let mut image_ids = std::collections::BTreeSet::new();
        for block in &self.blocks {
            match block {
                BlockSnapshot::Text(value) => {
                    if !text_ids.insert(value.id.get()) {
                        return Err(ModelError::InvalidDocument("duplicate text block id"));
                    }
                }
                BlockSnapshot::Table(value) => {
                    if !table_ids.insert(value.id.get()) {
                        return Err(ModelError::InvalidDocument("duplicate table id"));
                    }
                }
                BlockSnapshot::Image(value) => {
                    if !image_ids.insert(value.id.get()) {
                        return Err(ModelError::InvalidDocument("duplicate image id"));
                    }
                }
            }
            if let BlockSnapshot::Table(table) = block {
                if table.rows.is_empty() || table.columns.is_empty() {
                    return Err(ModelError::InvalidTable("table must be at least 1x1"));
                }
                for row in &table.rows {
                    if row.cells.len() != table.columns.len() {
                        return Err(ModelError::InvalidTable("table must be rectangular"));
                    }
                }
            }
            if let BlockSnapshot::Image(image) = block
                && (image.payload.bytes.is_empty()
                    || image.payload.dimensions.width == 0
                    || image.payload.dimensions.height == 0)
            {
                return Err(ModelError::InvalidImage(
                    "image payload and dimensions are required",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledTextFragment {
    pub runs: Vec<InlineRun>,
}
impl StyledTextFragment {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            runs: vec![InlineRun::new(text, TextStyle::default())],
        }
    }
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
    pub fn is_empty(&self) -> bool {
        self.runs.iter().all(|run| run.text.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardContent {
    Text(StyledTextFragment),
    Image(ImagePayload),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelError {
    InvalidValue(&'static str),
    InvalidOffset,
    InvalidRange,
    InvalidDocument(&'static str),
    InvalidTable(&'static str),
    InvalidImage(&'static str),
    NotFound(&'static str),
    IdExhausted,
}
impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidValue(value)
            | Self::InvalidDocument(value)
            | Self::InvalidTable(value)
            | Self::InvalidImage(value) => f.write_str(value),
            Self::InvalidOffset => f.write_str("text offset is outside the container"),
            Self::InvalidRange => f.write_str("text range is invalid"),
            Self::NotFound(value) => write!(f, "{value} was not found"),
            Self::IdExhausted => f.write_str("identifier space is exhausted"),
        }
    }
}
impl Error for ModelError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    Model(ModelError),
    InvalidSelection,
    Unsupported(&'static str),
    EmptyTransaction,
}
impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(value) => value.fmt(f),
            Self::InvalidSelection => f.write_str("selection is invalid"),
            Self::Unsupported(value) => f.write_str(value),
            Self::EmptyTransaction => f.write_str("transaction has no changes"),
        }
    }
}
impl Error for EditError {}
impl From<ModelError> for EditError {
    fn from(value: ModelError) -> Self {
        Self::Model(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchError {
    EmptyQuery,
    InvalidQuery(&'static str),
    Edit(EditError),
}
impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyQuery => f.write_str("search query must not be empty"),
            Self::InvalidQuery(value) => f.write_str(value),
            Self::Edit(value) => value.fmt(f),
        }
    }
}
impl Error for SearchError {}
impl From<EditError> for SearchError {
    fn from(value: EditError) -> Self {
        Self::Edit(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    Model(ModelError),
    Edit(EditError),
    Search(SearchError),
    UnsupportedFormat,
}
impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(value) => value.fmt(f),
            Self::Edit(value) => value.fmt(f),
            Self::Search(value) => value.fmt(f),
            Self::UnsupportedFormat => f.write_str("unsupported document format"),
        }
    }
}
impl Error for CoreError {}
impl From<ModelError> for CoreError {
    fn from(value: ModelError) -> Self {
        Self::Model(value)
    }
}
impl From<EditError> for CoreError {
    fn from(value: EditError) -> Self {
        Self::Edit(value)
    }
}
impl From<SearchError> for CoreError {
    fn from(value: SearchError) -> Self {
        Self::Search(value)
    }
}

fn validate_half_points(value: u16) -> Result<(), ModelError> {
    if (1..=16_384).contains(&value) {
        Ok(())
    } else {
        Err(ModelError::InvalidValue(
            "size must be between 1 and 16384 half-points",
        ))
    }
}

fn checked_next(value: &mut u64) -> Result<u64, ModelError> {
    let result = *value;
    *value = value.checked_add(1).ok_or(ModelError::IdExhausted)?;
    Ok(result)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextBlock {
    id: BlockId,
    kind: BlockKind,
    paragraph_style: ParagraphStyle,
    runs: Vec<InlineRun>,
}

impl TextBlock {
    fn empty(id: BlockId) -> Self {
        Self {
            id,
            kind: BlockKind::Paragraph,
            paragraph_style: ParagraphStyle::default(),
            runs: Vec::new(),
        }
    }
    fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
    fn len(&self) -> usize {
        self.runs.iter().map(|run| run.text.chars().count()).sum()
    }
    fn snapshot(&self) -> TextBlockSnapshot {
        TextBlockSnapshot {
            id: self.id,
            kind: self.kind.clone(),
            paragraph_style: self.paragraph_style.clone(),
            text: self.text(),
            runs: self.runs.clone(),
        }
    }
    fn chars(&self) -> Vec<(char, TextStyle)> {
        self.runs
            .iter()
            .flat_map(|run| run.text.chars().map(|ch| (ch, run.style.clone())))
            .collect()
    }
    fn from_chars(
        id: BlockId,
        kind: BlockKind,
        paragraph_style: ParagraphStyle,
        chars: &[(char, TextStyle)],
    ) -> Self {
        let mut runs: Vec<InlineRun> = Vec::new();
        for (ch, style) in chars {
            if let Some(last) = runs.last_mut()
                && last.style == *style
            {
                last.text.push(*ch);
                continue;
            }
            runs.push(InlineRun::new(ch.to_string(), style.clone()));
        }
        Self {
            id,
            kind,
            paragraph_style,
            runs,
        }
    }
    fn replace_range(
        &mut self,
        range: TextRange,
        replacement: &StyledTextFragment,
    ) -> Result<(), ModelError> {
        let range = range.ordered();
        let mut chars = self.chars();
        if range.end.get() > chars.len() {
            return Err(ModelError::InvalidRange);
        }
        let replacement_chars: Vec<(char, TextStyle)> = replacement
            .runs
            .iter()
            .flat_map(|run| run.text.chars().map(|ch| (ch, run.style.clone())))
            .collect();
        chars.splice(range.start.get()..range.end.get(), replacement_chars);
        *self = Self::from_chars(
            self.id,
            self.kind.clone(),
            self.paragraph_style.clone(),
            &chars,
        );
        Ok(())
    }
    fn style_range(
        &mut self,
        range: TextRange,
        patch: &TextStylePatch,
    ) -> Result<bool, ModelError> {
        let range = range.ordered();
        let mut chars = self.chars();
        if range.end.get() > chars.len() {
            return Err(ModelError::InvalidRange);
        }
        let mut changed = false;
        for (_, style) in chars
            .get_mut(range.start.get()..range.end.get())
            .ok_or(ModelError::InvalidRange)?
        {
            let next = patch.apply(style)?;
            if next != *style {
                *style = next;
                changed = true;
            }
        }
        *self = Self::from_chars(
            self.id,
            self.kind.clone(),
            self.paragraph_style.clone(),
            &chars,
        );
        Ok(changed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Cell {
    id: CellId,
    runs: Vec<InlineRun>,
    style: CellStyle,
    alignment: CellAlignment,
}
impl Cell {
    fn empty(id: CellId) -> Self {
        Self {
            id,
            runs: Vec::new(),
            style: CellStyle::default(),
            alignment: CellAlignment::default(),
        }
    }
    fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
    fn len(&self) -> usize {
        self.runs.iter().map(|run| run.text.chars().count()).sum()
    }
    fn snapshot(&self) -> TableCellSnapshot {
        TableCellSnapshot {
            id: self.id,
            text: self.text(),
            runs: self.runs.clone(),
            style: self.style,
            alignment: self.alignment,
        }
    }
    fn as_text_block(&self) -> TextBlock {
        TextBlock {
            id: BlockId::new(self.id.get()),
            kind: BlockKind::Paragraph,
            paragraph_style: ParagraphStyle::default(),
            runs: self.runs.clone(),
        }
    }
    fn replace_range(
        &mut self,
        range: TextRange,
        replacement: &StyledTextFragment,
    ) -> Result<(), ModelError> {
        let mut block = self.as_text_block();
        block.replace_range(range, replacement)?;
        self.runs = block.runs;
        Ok(())
    }
    fn style_range(
        &mut self,
        range: TextRange,
        patch: &TextStylePatch,
    ) -> Result<bool, ModelError> {
        let mut block = self.as_text_block();
        let changed = block.style_range(range, patch)?;
        self.runs = block.runs;
        Ok(changed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TableColumn {
    id: ColumnId,
    width_half_points: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct TableRow {
    id: RowId,
    cells: Vec<Cell>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Table {
    id: TableId,
    columns: Vec<TableColumn>,
    rows: Vec<TableRow>,
}
impl Table {
    fn snapshot(&self) -> TableSnapshot {
        TableSnapshot {
            id: self.id,
            columns: self
                .columns
                .iter()
                .map(|c| TableColumnSnapshot {
                    id: c.id,
                    width_half_points: c.width_half_points,
                })
                .collect(),
            rows: self
                .rows
                .iter()
                .map(|row| TableRowSnapshot {
                    id: row.id,
                    cells: row.cells.iter().map(Cell::snapshot).collect(),
                })
                .collect(),
        }
    }
    fn find_cell_mut(&mut self, id: CellId) -> Option<&mut Cell> {
        self.rows
            .iter_mut()
            .flat_map(|row| row.cells.iter_mut())
            .find(|cell| cell.id == id)
    }
    fn find_cell(&self, id: CellId) -> Option<&Cell> {
        self.rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .find(|cell| cell.id == id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Image {
    id: ImageId,
    payload: ImagePayload,
    layout: ImageLayout,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Block {
    Text(TextBlock),
    Table(Table),
    Image(Image),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    blocks: Vec<Block>,
    next_block: u64,
    next_table: u64,
    next_row: u64,
    next_column: u64,
    next_cell: u64,
    next_image: u64,
    next_list: u64,
}

impl Document {
    pub fn new() -> Self {
        Self {
            blocks: vec![Block::Text(TextBlock::empty(BlockId::new(1)))],
            next_block: 2,
            next_table: 1,
            next_row: 1,
            next_column: 1,
            next_cell: 1,
            next_image: 1,
            next_list: 1,
        }
    }
    fn snapshot(&self, revision: Revision) -> DocumentSnapshot {
        DocumentSnapshot {
            revision,
            blocks: self
                .blocks
                .iter()
                .map(|block| match block {
                    Block::Text(value) => BlockSnapshot::Text(value.snapshot()),
                    Block::Table(value) => BlockSnapshot::Table(value.snapshot()),
                    Block::Image(value) => BlockSnapshot::Image(ImageSnapshot {
                        id: value.id,
                        payload: value.payload.clone(),
                        layout: value.layout,
                    }),
                })
                .collect(),
        }
    }
    fn restore(&mut self, other: &Self) {
        self.blocks = other.blocks.clone();
        self.next_block = self.next_block.max(other.next_block);
        self.next_table = self.next_table.max(other.next_table);
        self.next_row = self.next_row.max(other.next_row);
        self.next_column = self.next_column.max(other.next_column);
        self.next_cell = self.next_cell.max(other.next_cell);
        self.next_image = self.next_image.max(other.next_image);
        self.next_list = self.next_list.max(other.next_list);
    }
    fn allocate_block(&mut self) -> Result<BlockId, ModelError> {
        Ok(BlockId::new(checked_next(&mut self.next_block)?))
    }
    fn allocate_table(&mut self) -> Result<TableId, ModelError> {
        Ok(TableId::new(checked_next(&mut self.next_table)?))
    }
    fn allocate_row(&mut self) -> Result<RowId, ModelError> {
        Ok(RowId::new(checked_next(&mut self.next_row)?))
    }
    fn allocate_column(&mut self) -> Result<ColumnId, ModelError> {
        Ok(ColumnId::new(checked_next(&mut self.next_column)?))
    }
    fn allocate_cell(&mut self) -> Result<CellId, ModelError> {
        Ok(CellId::new(checked_next(&mut self.next_cell)?))
    }
    fn allocate_image(&mut self) -> Result<ImageId, ModelError> {
        Ok(ImageId::new(checked_next(&mut self.next_image)?))
    }
    fn block_text_len(&self, id: BlockId) -> Option<usize> {
        self.blocks.iter().find_map(|block| match block {
            Block::Text(value) if value.id == id => Some(value.len()),
            _ => None,
        })
    }
    fn cell_len(&self, id: CellId) -> Option<usize> {
        self.blocks.iter().find_map(|block| match block {
            Block::Table(table) => table.find_cell(id).map(Cell::len),
            _ => None,
        })
    }
    fn text_len(&self, container: TextContainerId) -> Option<usize> {
        match container {
            TextContainerId::Block(id) => self.block_text_len(id),
            TextContainerId::Cell(id) => self.cell_len(id),
        }
    }
    fn validate_cursor(&self, cursor: Cursor) -> Result<(), EditError> {
        let len = self
            .text_len(cursor.container)
            .ok_or(EditError::Model(ModelError::NotFound("text container")))?;
        if cursor.offset.get() <= len {
            Ok(())
        } else {
            Err(EditError::Model(ModelError::InvalidOffset))
        }
    }
    fn block_index(&self, id: BlockId) -> Option<usize> {
        self.blocks
            .iter()
            .position(|block| matches!(block, Block::Text(value) if value.id == id))
    }
    fn table_mut(&mut self, id: TableId) -> Result<&mut Table, ModelError> {
        self.blocks
            .iter_mut()
            .find_map(|block| match block {
                Block::Table(table) if table.id == id => Some(table),
                _ => None,
            })
            .ok_or(ModelError::NotFound("table"))
    }
    fn table(&self, id: TableId) -> Result<&Table, ModelError> {
        self.blocks
            .iter()
            .find_map(|block| match block {
                Block::Table(table) if table.id == id => Some(table),
                _ => None,
            })
            .ok_or(ModelError::NotFound("table"))
    }
    fn image_mut(&mut self, id: ImageId) -> Result<&mut Image, ModelError> {
        self.blocks
            .iter_mut()
            .find_map(|block| match block {
                Block::Image(image) if image.id == id => Some(image),
                _ => None,
            })
            .ok_or(ModelError::NotFound("image"))
    }
    fn image(&self, id: ImageId) -> Result<&Image, ModelError> {
        self.blocks
            .iter()
            .find_map(|block| match block {
                Block::Image(image) if image.id == id => Some(image),
                _ => None,
            })
            .ok_or(ModelError::NotFound("image"))
    }
    fn cell_mut(&mut self, id: CellId) -> Result<&mut Cell, ModelError> {
        self.blocks
            .iter_mut()
            .find_map(|block| match block {
                Block::Table(table) => table.find_cell_mut(id),
                _ => None,
            })
            .ok_or(ModelError::NotFound("cell"))
    }
    fn cell(&self, id: CellId) -> Result<&Cell, ModelError> {
        self.blocks
            .iter()
            .find_map(|block| match block {
                Block::Table(table) => table.find_cell(id),
                _ => None,
            })
            .ok_or(ModelError::NotFound("cell"))
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn snapshot_public(&self) -> DocumentSnapshot {
        self.snapshot(Revision::new(0))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    InsertText {
        text: String,
    },
    DeleteRange {
        range: TextRange,
    },
    ReplaceRange {
        range: TextRange,
        text: String,
    },
    DeleteBackward,
    DeleteForward,
    SplitBlock,
    MergeWithPrevious,
    SetTextStyle {
        range: TextRange,
        patch: TextStylePatch,
    },
    SetParagraphStyle {
        block_id: BlockId,
        patch: ParagraphStylePatch,
    },
    SetBlockKind {
        block_id: BlockId,
        kind: BlockKind,
    },
    InsertTable {
        rows: usize,
        columns: usize,
    },
    AddTableRows {
        table_id: TableId,
        index: usize,
        count: usize,
    },
    RemoveTableRows {
        table_id: TableId,
        index: usize,
        count: usize,
    },
    AddTableColumns {
        table_id: TableId,
        index: usize,
        count: usize,
    },
    RemoveTableColumns {
        table_id: TableId,
        index: usize,
        count: usize,
    },
    ResizeTableColumn {
        table_id: TableId,
        column_id: ColumnId,
        width_half_points: u32,
    },
    SetCellStyle {
        cell_id: CellId,
        style: CellStyle,
    },
    SetCellAlignment {
        cell_id: CellId,
        alignment: CellAlignment,
    },
    SetCellBorders {
        cell_id: CellId,
        borders: Borders,
    },
    InsertImage {
        payload: ImagePayload,
    },
    DeleteImage {
        image_id: ImageId,
    },
    ResizeImage {
        image_id: ImageId,
        size: PixelSize,
    },
    SetImageAlignment {
        image_id: ImageId,
        alignment: Alignment,
    },
    SetImageAspectLock {
        image_id: ImageId,
        locked: bool,
    },
    Paste {
        content: ClipboardContent,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
}
impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            case_sensitive: true,
            whole_word: false,
        }
    }
    pub fn ascii_case_insensitive(mut self, value: bool) -> Self {
        self.case_sensitive = !value;
        self
    }
    pub fn case_sensitive(mut self, value: bool) -> Self {
        self.case_sensitive = value;
        self
    }
    pub fn whole_word(mut self, value: bool) -> Self {
        self.whole_word = value;
        self
    }
}
impl From<&str> for SearchQuery {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}
impl From<String> for SearchQuery {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}
impl From<&SearchQuery> for SearchQuery {
    fn from(value: &SearchQuery) -> Self {
        value.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    pub container: TextContainerId,
    pub range: TextRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentFormat {
    Docx,
    Pdf,
    Markdown,
    PlainText,
}

pub trait LayoutEngine {
    fn layout(&self, document: &DocumentSnapshot) -> Result<(), CoreError>;
}

pub fn document_to_plain_text(document: &DocumentSnapshot) -> String {
    let mut output = String::new();
    for (index, block) in document.blocks.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        match block {
            BlockSnapshot::Text(value) => {
                if matches!(&value.kind, BlockKind::PageBreak) {
                    output.push_str("[Page break]");
                } else {
                    output.push_str(&value.text);
                }
            }
            BlockSnapshot::Table(table) => {
                for (row_index, row) in table.rows.iter().enumerate() {
                    if row_index > 0 {
                        output.push('\n');
                    }
                    output.push_str(
                        &row.cells
                            .iter()
                            .map(|cell| cell.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\t"),
                    );
                }
            }
            BlockSnapshot::Image(_) => output.push_str("[Image]"),
        }
    }
    output
}

pub fn document_to_markdown(document: &DocumentSnapshot) -> String {
    let mut output = String::new();
    for (index, block) in document.blocks.iter().enumerate() {
        if index > 0 {
            output.push_str("\n\n");
        }
        match block {
            BlockSnapshot::Text(value) => match value.kind {
                BlockKind::Heading(level) => {
                    output.push_str(&"#".repeat(level.get() as usize));
                    output.push(' ');
                    output.push_str(&value.text);
                }
                BlockKind::ListItem {
                    kind: ListKind::Bullet,
                    ..
                } => {
                    output.push_str("- ");
                    output.push_str(&value.text);
                }
                BlockKind::ListItem {
                    kind: ListKind::Numbered,
                    ..
                } => {
                    output.push_str("1. ");
                    output.push_str(&value.text);
                }
                BlockKind::Paragraph => output.push_str(&value.text),
                BlockKind::PageBreak => output.push_str("[Page break]"),
            },
            BlockSnapshot::Table(table) => {
                if let Some(first) = table.rows.first() {
                    output.push('|');
                    for cell in &first.cells {
                        output.push(' ');
                        output.push_str(&cell.text);
                        output.push_str(" |");
                    }
                    output.push('\n');
                    output.push('|');
                    for _ in &first.cells {
                        output.push_str(" --- |");
                    }
                    for row in table.rows.iter().skip(1) {
                        output.push('\n');
                        output.push('|');
                        for cell in &row.cells {
                            output.push(' ');
                            output.push_str(&cell.text);
                            output.push_str(" |");
                        }
                    }
                }
            }
            BlockSnapshot::Image(_) => output.push_str("![image](image)"),
        }
    }
    output
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryEntry {
    before: Document,
    after: Document,
    selection_before: Selection,
    selection_after: Selection,
}

pub struct Editor {
    document: Document,
    selection: Selection,
    revision: Revision,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    pub fn new() -> Self {
        let document = Document::new();
        let id = match document.blocks.first() {
            Some(Block::Text(value)) => value.id,
            _ => BlockId::new(1),
        };
        Self {
            document,
            selection: Selection::collapsed(Cursor::new(
                TextContainerId::Block(id),
                TextOffset::new(0),
            )),
            revision: Revision::new(0),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn snapshot(&self) -> DocumentSnapshot {
        self.document.snapshot(self.revision)
    }
    pub fn selection(&self) -> Selection {
        self.selection
    }
    pub fn revision(&self) -> Revision {
        self.revision
    }
    pub fn set_selection(&mut self, selection: Selection) -> Result<(), EditError> {
        self.document.validate_cursor(selection.anchor)?;
        self.document.validate_cursor(selection.active)?;
        if selection.anchor.container() != selection.active.container() {
            return Err(EditError::InvalidSelection);
        }
        self.selection = selection;
        Ok(())
    }
    fn commit_state(
        &mut self,
        mut document: Document,
        selection: Selection,
    ) -> Result<(), EditError> {
        if self.document.blocks == document.blocks && self.selection == selection {
            return Ok(());
        }
        let next_revision = self.next_revision()?;
        let before = self.document.clone();
        let selection_before = self.selection;
        document.next_block = document.next_block.max(self.document.next_block);
        document.next_table = document.next_table.max(self.document.next_table);
        document.next_row = document.next_row.max(self.document.next_row);
        document.next_column = document.next_column.max(self.document.next_column);
        document.next_cell = document.next_cell.max(self.document.next_cell);
        document.next_image = document.next_image.max(self.document.next_image);
        document.next_list = document.next_list.max(self.document.next_list);
        self.undo.push(HistoryEntry {
            before,
            after: document.clone(),
            selection_before,
            selection_after: selection,
        });
        self.redo.clear();
        self.document = document;
        self.selection = selection;
        self.revision = next_revision;
        Ok(())
    }
    fn next_revision(&self) -> Result<Revision, EditError> {
        self.revision
            .get()
            .checked_add(1)
            .map(Revision::new)
            .ok_or(EditError::Model(ModelError::IdExhausted))
    }
    pub fn execute(&mut self, command: Command) -> Result<(), EditError> {
        let mut document = self.document.clone();
        let mut selection = self.selection;
        apply_command(&mut document, &mut selection, command)?;
        document.validate_cursor(selection.anchor)?;
        document.validate_cursor(selection.active)?;
        self.commit_state(document, selection)
    }
    pub fn begin_transaction(&mut self) -> Transaction<'_> {
        let document = self.document.clone();
        let selection = self.selection;
        Transaction {
            editor: self,
            document,
            selection,
            finished: false,
        }
    }
    pub fn undo(&mut self) -> Result<bool, EditError> {
        if self.undo.is_empty() {
            return Ok(false);
        }
        let next_revision = self.next_revision()?;
        let Some(entry) = self.undo.pop() else {
            return Ok(false);
        };
        let current = self.document.clone();
        let current_selection = self.selection;
        self.document.restore(&entry.before);
        self.selection = entry.selection_before;
        self.redo.push(HistoryEntry {
            before: entry.before,
            after: current,
            selection_before: entry.selection_before,
            selection_after: current_selection,
        });
        self.revision = next_revision;
        Ok(true)
    }
    pub fn redo(&mut self) -> Result<bool, EditError> {
        if self.redo.is_empty() {
            return Ok(false);
        }
        let next_revision = self.next_revision()?;
        let Some(entry) = self.redo.pop() else {
            return Ok(false);
        };
        let current = self.document.clone();
        let current_selection = self.selection;
        self.document.restore(&entry.after);
        self.selection = entry.selection_after;
        self.undo.push(HistoryEntry {
            before: current,
            after: entry.after,
            selection_before: current_selection,
            selection_after: entry.selection_after,
        });
        self.revision = next_revision;
        Ok(true)
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    /// Discards undo and redo entries without changing the current editor state.
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
    pub fn replace_document(&mut self, snapshot: DocumentSnapshot) -> Result<(), EditError> {
        snapshot.validate()?;
        let document = document_from_snapshot(&snapshot)?;
        let first = first_cursor(&document).ok_or(EditError::InvalidSelection)?;
        self.commit_state(document, first)
    }
    pub fn copy(&self) -> Option<StyledTextFragment> {
        let (container, range) = self.selection.range()?;
        if range.is_collapsed() {
            return None;
        }
        fragment_for_range(&self.document, container, range).ok()
    }
    pub fn copy_image(&self, image_id: ImageId) -> Result<ImagePayload, EditError> {
        Ok(self.document.image(image_id)?.payload.clone())
    }
    pub fn find<Q: Into<SearchQuery>>(&self, query: Q) -> Result<Vec<SearchMatch>, SearchError> {
        find_in_document(&self.document, query.into())
    }
    pub fn replace_all<Q: Into<SearchQuery>>(
        &mut self,
        query: Q,
        replacement: &str,
    ) -> Result<usize, SearchError> {
        let query = query.into();
        let matches = find_in_document(&self.document, query)?;
        if matches.is_empty() {
            return Ok(0);
        }
        let mut document = self.document.clone();
        let mut selection = self.selection;
        for item in matches.iter().rev() {
            let fragment = StyledTextFragment::plain(replacement);
            replace_in_container(&mut document, item.container, item.range, &fragment)?;
            selection = Selection::collapsed(Cursor::new(
                item.container,
                TextOffset::new(item.range.start.get() + replacement.chars().count()),
            ));
        }
        self.commit_state(document, selection)?;
        Ok(matches.len())
    }
}

pub struct Transaction<'a> {
    editor: &'a mut Editor,
    document: Document,
    selection: Selection,
    finished: bool,
}
impl Transaction<'_> {
    /// Changes only the staged selection; rollback leaves the editor untouched.
    pub fn set_selection(&mut self, selection: Selection) -> Result<(), EditError> {
        self.document.validate_cursor(selection.anchor)?;
        self.document.validate_cursor(selection.active)?;
        if selection.anchor.container() != selection.active.container() {
            return Err(EditError::InvalidSelection);
        }
        self.selection = selection;
        Ok(())
    }

    /// A rejected command leaves the previously staged state available for retry.
    pub fn execute(&mut self, command: Command) -> Result<(), EditError> {
        let mut document = self.document.clone();
        let mut selection = self.selection;
        apply_command(&mut document, &mut selection, command)?;
        document.validate_cursor(selection.anchor)?;
        document.validate_cursor(selection.active)?;
        self.document = document;
        self.selection = selection;
        Ok(())
    }
    pub fn selection(&self) -> Selection {
        self.selection
    }
    pub fn commit(mut self) -> Result<(), EditError> {
        self.finished = true;
        let document = self.document.clone();
        let selection = self.selection;
        self.editor.commit_state(document, selection)
    }
    pub fn rollback(mut self) {
        self.finished = true;
    }
}
impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.finished {}
    }
}

fn first_cursor(document: &Document) -> Option<Selection> {
    for block in &document.blocks {
        match block {
            Block::Text(block) => {
                return Some(Selection::collapsed(Cursor::new(
                    TextContainerId::Block(block.id),
                    TextOffset::new(0),
                )));
            }
            Block::Table(table) => {
                if let Some(cell) = table.rows.first().and_then(|row| row.cells.first()) {
                    return Some(Selection::collapsed(Cursor::new(
                        TextContainerId::Cell(cell.id),
                        TextOffset::new(0),
                    )));
                }
            }
            Block::Image(_) => {}
        }
    }
    None
}

fn fragment_for_range(
    document: &Document,
    container: TextContainerId,
    range: TextRange,
) -> Result<StyledTextFragment, EditError> {
    let range = range.ordered();
    match container {
        TextContainerId::Block(id) => {
            let block = document
                .blocks
                .iter()
                .find_map(|block| match block {
                    Block::Text(value) if value.id == id => Some(value),
                    _ => None,
                })
                .ok_or(EditError::Model(ModelError::NotFound("text block")))?;
            let chars = block.chars();
            if range.end.get() > chars.len() {
                return Err(EditError::Model(ModelError::InvalidRange));
            }
            Ok(fragment_from_chars(
                &chars[range.start.get()..range.end.get()],
            ))
        }
        TextContainerId::Cell(id) => {
            let cell = document.cell(id)?;
            let chars = cell.as_text_block().chars();
            if range.end.get() > chars.len() {
                return Err(EditError::Model(ModelError::InvalidRange));
            }
            Ok(fragment_from_chars(
                &chars[range.start.get()..range.end.get()],
            ))
        }
    }
}
fn fragment_from_chars(chars: &[(char, TextStyle)]) -> StyledTextFragment {
    let mut runs: Vec<InlineRun> = Vec::new();
    for (ch, style) in chars {
        if let Some(last) = runs.last_mut()
            && last.style == *style
        {
            last.text.push(*ch);
            continue;
        }
        runs.push(InlineRun::new(ch.to_string(), style.clone()));
    }
    StyledTextFragment { runs }
}

fn replace_in_container(
    document: &mut Document,
    container: TextContainerId,
    range: TextRange,
    fragment: &StyledTextFragment,
) -> Result<(), EditError> {
    match container {
        TextContainerId::Block(id) => {
            let block = document
                .blocks
                .iter_mut()
                .find_map(|block| match block {
                    Block::Text(value) if value.id == id => Some(value),
                    _ => None,
                })
                .ok_or(EditError::Model(ModelError::NotFound("text block")))?;
            block.replace_range(range, fragment)?;
        }
        TextContainerId::Cell(id) => document.cell_mut(id)?.replace_range(range, fragment)?,
    }
    Ok(())
}

fn apply_command(
    document: &mut Document,
    selection: &mut Selection,
    command: Command,
) -> Result<(), EditError> {
    match command {
        Command::InsertText { text } => {
            let (container, range) = selection.range().ok_or(EditError::InvalidSelection)?;
            let end = range.start.get() + text.chars().count();
            replace_in_container(document, container, range, &StyledTextFragment::plain(text))?;
            *selection = Selection::collapsed(Cursor::new(container, TextOffset::new(end)));
        }
        Command::DeleteRange { range } => {
            let container = selection.anchor().container();
            if selection.anchor().container() != selection.active().container() {
                return Err(EditError::InvalidSelection);
            }
            let range = range.ordered();
            replace_in_container(
                document,
                container,
                range,
                &StyledTextFragment { runs: Vec::new() },
            )?;
            *selection = Selection::collapsed(Cursor::new(container, range.start));
        }
        Command::ReplaceRange { range, text } => {
            let container = selection.anchor().container();
            let range = range.ordered();
            replace_in_container(
                document,
                container,
                range,
                &StyledTextFragment::plain(text.clone()),
            )?;
            *selection = Selection::collapsed(Cursor::new(
                container,
                TextOffset::new(range.start.get() + text.chars().count()),
            ));
        }
        Command::DeleteBackward => delete_backward(document, selection)?,
        Command::DeleteForward => delete_forward(document, selection)?,
        Command::SplitBlock => split_block(document, selection)?,
        Command::MergeWithPrevious => merge_previous(document, selection)?,
        Command::SetTextStyle { range: _, patch } => {
            let (container, range) = selection.range().ok_or(EditError::InvalidSelection)?;
            match container {
                TextContainerId::Block(id) => {
                    let block = document
                        .blocks
                        .iter_mut()
                        .find_map(|block| match block {
                            Block::Text(value) if value.id == id => Some(value),
                            _ => None,
                        })
                        .ok_or(EditError::Model(ModelError::NotFound("text block")))?;
                    block.style_range(range, &patch)?;
                }
                TextContainerId::Cell(id) => {
                    document.cell_mut(id)?.style_range(range, &patch)?;
                }
            }
        }
        Command::SetParagraphStyle { block_id, patch } => {
            let block = document
                .blocks
                .iter_mut()
                .find_map(|block| match block {
                    Block::Text(value) if value.id == block_id => Some(value),
                    _ => None,
                })
                .ok_or(EditError::Model(ModelError::NotFound("text block")))?;
            block.paragraph_style.apply_patch(&patch);
        }
        Command::SetBlockKind { block_id, kind } => {
            if let BlockKind::Heading(level) = kind {
                HeadingLevel::new(level.get())?;
            }
            let block = document
                .blocks
                .iter_mut()
                .find_map(|block| match block {
                    Block::Text(value) if value.id == block_id => Some(value),
                    _ => None,
                })
                .ok_or(EditError::Model(ModelError::NotFound("text block")))?;
            block.kind = kind;
        }
        Command::InsertTable { rows, columns } => insert_table(document, selection, rows, columns)?,
        Command::AddTableRows {
            table_id,
            index,
            count,
        } => add_rows(document, table_id, index, count)?,
        Command::RemoveTableRows {
            table_id,
            index,
            count,
        } => remove_rows(document, table_id, index, count)?,
        Command::AddTableColumns {
            table_id,
            index,
            count,
        } => add_columns(document, table_id, index, count)?,
        Command::RemoveTableColumns {
            table_id,
            index,
            count,
        } => remove_columns(document, table_id, index, count)?,
        Command::ResizeTableColumn {
            table_id,
            column_id,
            width_half_points,
        } => {
            if width_half_points == 0 {
                return Err(EditError::Model(ModelError::InvalidValue(
                    "column width must be non-zero",
                )));
            }
            let table = document.table_mut(table_id)?;
            let column = table
                .columns
                .iter_mut()
                .find(|column| column.id == column_id)
                .ok_or(ModelError::NotFound("column"))?;
            column.width_half_points = width_half_points;
        }
        Command::SetCellStyle { cell_id, style } => {
            document.cell_mut(cell_id)?.style = style;
        }
        Command::SetCellAlignment { cell_id, alignment } => {
            document.cell_mut(cell_id)?.alignment = alignment;
        }
        Command::SetCellBorders { cell_id, borders } => {
            document.cell_mut(cell_id)?.style.borders = borders;
        }
        Command::InsertImage { payload } => insert_image(document, selection, payload)?,
        Command::DeleteImage { image_id } => delete_image(document, selection, image_id)?,
        Command::ResizeImage { image_id, size } => resize_image(document, image_id, size)?,
        Command::SetImageAlignment {
            image_id,
            alignment,
        } => {
            document.image_mut(image_id)?.layout.alignment = alignment;
        }
        Command::SetImageAspectLock { image_id, locked } => {
            document.image_mut(image_id)?.layout.aspect_locked = locked;
        }
        Command::Paste { content } => paste(document, selection, content)?,
    }
    Ok(())
}

fn delete_backward(document: &mut Document, selection: &mut Selection) -> Result<(), EditError> {
    let (container, range) = selection.range().ok_or(EditError::InvalidSelection)?;
    if !range.is_collapsed() {
        replace_in_container(
            document,
            container,
            range,
            &StyledTextFragment { runs: Vec::new() },
        )?;
        *selection = Selection::collapsed(Cursor::new(container, range.start));
        return Ok(());
    }
    if range.start.get() == 0 {
        if let TextContainerId::Block(id) = container {
            let index = document
                .block_index(id)
                .ok_or(EditError::Model(ModelError::NotFound("text block")))?;
            if let Some(previous) = index.checked_sub(1) {
                join_adjacent_paragraphs(document, selection, previous);
            }
        }
        return Ok(());
    }
    let range = TextRange::new(TextOffset::new(range.start.get() - 1), range.start);
    replace_in_container(
        document,
        container,
        range,
        &StyledTextFragment { runs: Vec::new() },
    )?;
    *selection = Selection::collapsed(Cursor::new(container, range.start));
    Ok(())
}
fn delete_forward(document: &mut Document, selection: &mut Selection) -> Result<(), EditError> {
    let (container, range) = selection.range().ok_or(EditError::InvalidSelection)?;
    if !range.is_collapsed() {
        replace_in_container(
            document,
            container,
            range,
            &StyledTextFragment { runs: Vec::new() },
        )?;
        *selection = Selection::collapsed(Cursor::new(container, range.start));
        return Ok(());
    }
    let len = document
        .text_len(container)
        .ok_or(EditError::Model(ModelError::NotFound("text container")))?;
    if range.start.get() >= len {
        if let TextContainerId::Block(id) = container
            && range.start.get() == len
        {
            let index = document
                .block_index(id)
                .ok_or(EditError::Model(ModelError::NotFound("text block")))?;
            join_adjacent_paragraphs(document, selection, index);
        }
        return Ok(());
    }
    let range = TextRange::new(range.start, TextOffset::new(range.start.get() + 1));
    replace_in_container(
        document,
        container,
        range,
        &StyledTextFragment { runs: Vec::new() },
    )?;
    Ok(())
}

/// Boundary deletion joins only immediate ordinary paragraphs. The explicit
/// MergeWithPrevious command intentionally retains its broader legacy contract.
fn join_adjacent_paragraphs(document: &mut Document, selection: &mut Selection, left_index: usize) {
    let Some(right_index) = left_index.checked_add(1) else {
        return;
    };
    let Some(pair) = document.blocks.get_mut(left_index..=right_index) else {
        return;
    };
    let [Block::Text(left), Block::Text(right)] = pair else {
        return;
    };
    if left.kind != BlockKind::Paragraph || right.kind != BlockKind::Paragraph {
        return;
    }

    let cursor = Cursor::new(TextContainerId::Block(left.id), TextOffset::new(left.len()));
    // Retain exact styled runs, including adjacent equal styles and empty runs.
    // All boundary checks precede mutation; no fallible operation follows it.
    left.runs.append(&mut right.runs);
    document.blocks.remove(right_index);
    *selection = Selection::collapsed(cursor);
}

fn split_block(document: &mut Document, selection: &mut Selection) -> Result<(), EditError> {
    let (container, range) = selection.range().ok_or(EditError::InvalidSelection)?;
    if !range.is_collapsed() || !matches!(container, TextContainerId::Block(_)) {
        return Err(EditError::Unsupported(
            "only a collapsed block selection can split",
        ));
    }
    let id = match container {
        TextContainerId::Block(id) => id,
        TextContainerId::Cell(_) => return Err(EditError::Unsupported("cells cannot be split")),
    };
    let index = document
        .block_index(id)
        .ok_or(EditError::Model(ModelError::NotFound("text block")))?;
    let block = match document.blocks.remove(index) {
        Block::Text(block) => block,
        other => {
            document.blocks.insert(index, other);
            return Err(EditError::Unsupported("only text blocks can be split"));
        }
    };
    let chars = block.chars();
    let at = range.start.get();
    if at > chars.len() {
        document.blocks.insert(index, Block::Text(block));
        return Err(EditError::Model(ModelError::InvalidRange));
    }
    let new_id = document.allocate_block()?;
    let left = TextBlock::from_chars(
        block.id,
        block.kind.clone(),
        block.paragraph_style.clone(),
        &chars[..at],
    );
    let right = TextBlock::from_chars(new_id, block.kind, block.paragraph_style, &chars[at..]);
    document.blocks.insert(index, Block::Text(left));
    document.blocks.insert(index + 1, Block::Text(right));
    *selection = Selection::collapsed(Cursor::new(
        TextContainerId::Block(new_id),
        TextOffset::new(0),
    ));
    Ok(())
}

fn merge_previous(document: &mut Document, selection: &mut Selection) -> Result<(), EditError> {
    let (container, range) = selection.range().ok_or(EditError::InvalidSelection)?;
    let id = match container {
        TextContainerId::Block(id) if range.is_collapsed() => id,
        _ => {
            return Err(EditError::Unsupported(
                "only a collapsed block selection can merge",
            ));
        }
    };
    let index = document
        .block_index(id)
        .ok_or(EditError::Model(ModelError::NotFound("text block")))?;
    if index == 0 {
        return Ok(());
    }
    let previous = match document.blocks.get(index - 1) {
        Some(Block::Text(block)) => block.clone(),
        _ => return Err(EditError::Unsupported("previous block is not text")),
    };
    let current = match document.blocks.get(index) {
        Some(Block::Text(block)) => block.clone(),
        _ => return Err(EditError::Unsupported("current block is not text")),
    };
    let offset = previous.len();
    let mut chars = previous.chars();
    chars.extend(current.chars());
    document.blocks[index - 1] = Block::Text(TextBlock::from_chars(
        previous.id,
        previous.kind,
        previous.paragraph_style,
        &chars,
    ));
    document.blocks.remove(index);
    *selection = Selection::collapsed(Cursor::new(
        TextContainerId::Block(previous.id),
        TextOffset::new(offset + range.start.get()),
    ));
    Ok(())
}

fn insert_table(
    document: &mut Document,
    selection: &mut Selection,
    rows: usize,
    columns: usize,
) -> Result<(), EditError> {
    if rows == 0 || columns == 0 {
        return Err(EditError::Model(ModelError::InvalidTable(
            "table must be at least 1x1",
        )));
    }
    let table_id = document.allocate_table()?;
    let mut table = Table {
        id: table_id,
        columns: Vec::new(),
        rows: Vec::new(),
    };
    for _ in 0..columns {
        table.columns.push(TableColumn {
            id: document.allocate_column()?,
            width_half_points: 1440,
        });
    }
    for _ in 0..rows {
        let row_id = document.allocate_row()?;
        let mut cells = Vec::new();
        for _ in 0..columns {
            cells.push(Cell::empty(document.allocate_cell()?));
        }
        table.rows.push(TableRow { id: row_id, cells });
    }
    let index = match selection.anchor().container() {
        TextContainerId::Block(id) => document.block_index(id).unwrap_or(document.blocks.len()),
        TextContainerId::Cell(_) => document.blocks.len(),
    };
    document.blocks.insert(index, Block::Table(table));
    let cell = match document.blocks.get(index) {
        Some(Block::Table(table)) => table.rows[0].cells[0].id,
        _ => {
            return Err(EditError::Model(ModelError::InvalidTable(
                "table insertion failed",
            )));
        }
    };
    *selection = Selection::collapsed(Cursor::new(TextContainerId::Cell(cell), TextOffset::new(0)));
    Ok(())
}

fn add_rows(
    document: &mut Document,
    table_id: TableId,
    index: usize,
    count: usize,
) -> Result<(), EditError> {
    let table = document.table(table_id)?;
    if index > table.rows.len() {
        return Err(EditError::Model(ModelError::InvalidRange));
    }
    let columns = table.columns.len();
    let mut rows = Vec::new();
    for _ in 0..count {
        let id = document.allocate_row()?;
        let mut cells = Vec::new();
        for _ in 0..columns {
            cells.push(Cell::empty(document.allocate_cell()?));
        }
        rows.push(TableRow { id, cells });
    }
    let table = document.table_mut(table_id)?;
    table.rows.splice(index..index, rows);
    Ok(())
}
fn remove_rows(
    document: &mut Document,
    table_id: TableId,
    index: usize,
    count: usize,
) -> Result<(), EditError> {
    let table = document.table(table_id)?;
    if index > table.rows.len()
        || count > table.rows.len().saturating_sub(index)
        || table.rows.len() - count == 0
    {
        return Err(EditError::Model(ModelError::InvalidTable(
            "table must retain at least one row",
        )));
    }
    document
        .table_mut(table_id)?
        .rows
        .drain(index..index + count);
    Ok(())
}
fn add_columns(
    document: &mut Document,
    table_id: TableId,
    index: usize,
    count: usize,
) -> Result<(), EditError> {
    let table = document.table(table_id)?;
    if index > table.columns.len() {
        return Err(EditError::Model(ModelError::InvalidRange));
    }
    let mut columns = Vec::new();
    for _ in 0..count {
        columns.push(TableColumn {
            id: document.allocate_column()?,
            width_half_points: 1440,
        });
    }
    let mut cell_ids = Vec::new();
    for _ in 0..(count * document.table(table_id)?.rows.len()) {
        cell_ids.push(document.allocate_cell()?);
    }
    let table = document.table_mut(table_id)?;
    table.columns.splice(index..index, columns);
    let mut cell_ids = cell_ids.into_iter();
    for row in &mut table.rows {
        let mut cells = Vec::new();
        for _ in 0..count {
            let Some(cell_id) = cell_ids.next() else {
                return Err(EditError::Model(ModelError::IdExhausted));
            };
            cells.push(Cell::empty(cell_id));
        }
        row.cells.splice(index..index, cells);
    }
    Ok(())
}
fn remove_columns(
    document: &mut Document,
    table_id: TableId,
    index: usize,
    count: usize,
) -> Result<(), EditError> {
    let table = document.table(table_id)?;
    if index > table.columns.len()
        || count > table.columns.len().saturating_sub(index)
        || table.columns.len() - count == 0
    {
        return Err(EditError::Model(ModelError::InvalidTable(
            "table must retain at least one column",
        )));
    }
    let table = document.table_mut(table_id)?;
    table.columns.drain(index..index + count);
    for row in &mut table.rows {
        row.cells.drain(index..index + count);
    }
    Ok(())
}

fn insert_image(
    document: &mut Document,
    selection: &mut Selection,
    payload: ImagePayload,
) -> Result<(), EditError> {
    let image_id = document.allocate_image()?;
    let layout = ImageLayout {
        size: payload.dimensions,
        ..ImageLayout::default()
    };
    let index = match selection.anchor().container() {
        TextContainerId::Block(id) => document.block_index(id).unwrap_or(document.blocks.len()),
        TextContainerId::Cell(_) => document.blocks.len(),
    };
    document.blocks.insert(
        index,
        Block::Image(Image {
            id: image_id,
            payload,
            layout,
        }),
    );
    Ok(())
}
fn delete_image(
    document: &mut Document,
    selection: &mut Selection,
    image_id: ImageId,
) -> Result<(), EditError> {
    let index = document
        .blocks
        .iter()
        .position(|block| matches!(block, Block::Image(image) if image.id == image_id))
        .ok_or(EditError::Model(ModelError::NotFound("image")))?;
    document.blocks.remove(index);
    if document.blocks.is_empty() {
        let id = document.allocate_block()?;
        document.blocks.push(Block::Text(TextBlock::empty(id)));
    }
    if let Some(next) = first_cursor(document) {
        *selection = next;
    }
    Ok(())
}
fn resize_image(
    document: &mut Document,
    image_id: ImageId,
    size: PixelSize,
) -> Result<(), EditError> {
    let image = document.image_mut(image_id)?;
    let old = image.layout.size;
    let new_size = if image.layout.aspect_locked {
        proportional_size(old, size)?
    } else {
        size
    };
    image.layout.size = new_size;
    Ok(())
}
fn proportional_size(old: PixelSize, requested: PixelSize) -> Result<PixelSize, ModelError> {
    if old.width == 0 || old.height == 0 || requested.width == 0 || requested.height == 0 {
        return Err(ModelError::InvalidValue(
            "image dimensions must be non-zero",
        ));
    }
    let width_limited = (requested.width as u128) * (old.height as u128)
        <= (requested.height as u128) * (old.width as u128);
    if width_limited {
        let height = ((requested.width as u128) * (old.height as u128) + (old.width as u128 / 2))
            / old.width as u128;
        Ok(PixelSize {
            width: requested.width,
            height: height.clamp(1, u32::MAX as u128) as u32,
        })
    } else {
        let width = ((requested.height as u128) * (old.width as u128) + (old.height as u128 / 2))
            / old.height as u128;
        Ok(PixelSize {
            width: width.clamp(1, u32::MAX as u128) as u32,
            height: requested.height,
        })
    }
}
fn document_from_snapshot(snapshot: &DocumentSnapshot) -> Result<Document, EditError> {
    let mut document = Document::new();
    document.blocks.clear();
    document.next_block = 1;
    document.next_table = 1;
    document.next_row = 1;
    document.next_column = 1;
    document.next_cell = 1;
    document.next_image = 1;
    document.next_list = 1;
    for block in &snapshot.blocks {
        match block {
            BlockSnapshot::Text(value) => {
                document.next_block = document.next_block.max(
                    value
                        .id
                        .get()
                        .checked_add(1)
                        .ok_or(EditError::Model(ModelError::IdExhausted))?,
                );
                document.next_list = document.next_list.max(match value.kind {
                    BlockKind::ListItem { list_id, .. } => list_id
                        .get()
                        .checked_add(1)
                        .ok_or(EditError::Model(ModelError::IdExhausted))?,
                    _ => 1,
                });
                document.blocks.push(Block::Text(TextBlock {
                    id: value.id,
                    kind: value.kind.clone(),
                    paragraph_style: value.paragraph_style.clone(),
                    runs: value.runs.clone(),
                }));
            }
            BlockSnapshot::Table(value) => {
                if value.rows.is_empty() || value.columns.is_empty() {
                    return Err(EditError::Model(ModelError::InvalidTable(
                        "table must be at least 1x1",
                    )));
                }
                let mut table = Table {
                    id: value.id,
                    columns: Vec::new(),
                    rows: Vec::new(),
                };
                document.next_table = document.next_table.max(
                    value
                        .id
                        .get()
                        .checked_add(1)
                        .ok_or(EditError::Model(ModelError::IdExhausted))?,
                );
                for column in &value.columns {
                    document.next_column = document.next_column.max(
                        column
                            .id
                            .get()
                            .checked_add(1)
                            .ok_or(EditError::Model(ModelError::IdExhausted))?,
                    );
                    table.columns.push(TableColumn {
                        id: column.id,
                        width_half_points: column.width_half_points,
                    });
                }
                for row in &value.rows {
                    document.next_row = document.next_row.max(
                        row.id
                            .get()
                            .checked_add(1)
                            .ok_or(EditError::Model(ModelError::IdExhausted))?,
                    );
                    let cells = row
                        .cells
                        .iter()
                        .map(|cell| {
                            document.next_cell =
                                document.next_cell.max(cell.id.get().saturating_add(1));
                            Cell {
                                id: cell.id,
                                runs: cell.runs.clone(),
                                style: cell.style,
                                alignment: cell.alignment,
                            }
                        })
                        .collect();
                    table.rows.push(TableRow { id: row.id, cells });
                }
                document.blocks.push(Block::Table(table));
            }
            BlockSnapshot::Image(value) => {
                document.next_image = document.next_image.max(
                    value
                        .id
                        .get()
                        .checked_add(1)
                        .ok_or(EditError::Model(ModelError::IdExhausted))?,
                );
                document.blocks.push(Block::Image(Image {
                    id: value.id,
                    payload: value.payload.clone(),
                    layout: value.layout,
                }));
            }
        }
    }
    if document.blocks.is_empty() {
        return Err(EditError::Model(ModelError::InvalidDocument(
            "document must contain a block",
        )));
    }
    Ok(document)
}

fn ascii_equal(left: &[char], right: &[char], case_sensitive: bool) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(a, b)| {
            if case_sensitive {
                a == b
            } else {
                a.eq_ignore_ascii_case(b)
            }
        })
}
fn is_ascii_word(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_'
}
fn find_in_text(
    chars: &[(char, TextStyle)],
    container: TextContainerId,
    query: &SearchQuery,
) -> Result<Vec<SearchMatch>, SearchError> {
    if query.text.is_empty() {
        return Err(SearchError::EmptyQuery);
    }
    let haystack: Vec<char> = chars.iter().map(|(ch, _)| *ch).collect();
    let needle: Vec<char> = query.text.chars().collect();
    let mut matches = Vec::new();
    if needle.is_empty() {
        return Err(SearchError::EmptyQuery);
    }
    for start in 0..=haystack.len().saturating_sub(needle.len()) {
        let end = start + needle.len();
        if !ascii_equal(&haystack[start..end], &needle, query.case_sensitive) {
            continue;
        }
        if query.whole_word
            && ((start > 0 && is_ascii_word(haystack[start - 1]))
                || (end < haystack.len() && is_ascii_word(haystack[end])))
        {
            continue;
        }
        matches.push(SearchMatch {
            container,
            range: TextRange::new(TextOffset::new(start), TextOffset::new(end)),
        });
    }
    Ok(matches)
}
fn find_in_document(
    document: &Document,
    query: SearchQuery,
) -> Result<Vec<SearchMatch>, SearchError> {
    if query.text.is_empty() {
        return Err(SearchError::EmptyQuery);
    }
    let mut result = Vec::new();
    for block in &document.blocks {
        match block {
            Block::Text(value) => result.extend(find_in_text(
                &value.chars(),
                TextContainerId::Block(value.id),
                &query,
            )?),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        result.extend(find_in_text(
                            &cell.as_text_block().chars(),
                            TextContainerId::Cell(cell.id),
                            &query,
                        )?);
                    }
                }
            }
            Block::Image(_) => {}
        }
    }
    Ok(result)
}

fn paste(
    document: &mut Document,
    selection: &mut Selection,
    content: ClipboardContent,
) -> Result<(), EditError> {
    match content {
        ClipboardContent::Text(fragment) => {
            let (container, range) = selection.range().ok_or(EditError::InvalidSelection)?;
            let end = range.start.get() + fragment.text().chars().count();
            replace_in_container(document, container, range, &fragment)?;
            *selection = Selection::collapsed(Cursor::new(container, TextOffset::new(end)));
        }
        ClipboardContent::Image(payload) => insert_image(document, selection, payload)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transactional_editor_state(
        editor: &Editor,
    ) -> (
        Document,
        Selection,
        Revision,
        Vec<HistoryEntry>,
        Vec<HistoryEntry>,
    ) {
        (
            editor.document.clone(),
            editor.selection,
            editor.revision,
            editor.undo.clone(),
            editor.redo.clone(),
        )
    }

    fn transactional_editor_with_redo() -> Editor {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: "A🙂B".to_owned(),
            })
            .unwrap();
        editor
            .execute(Command::InsertText {
                text: " second".to_owned(),
            })
            .unwrap();
        assert!(editor.undo().unwrap());
        assert!(editor.can_undo());
        assert!(editor.can_redo());
        editor.revision = Revision::new(u64::MAX);
        editor
    }

    #[test]
    fn transactional_revision_exhaustion_preserves_execute_state_and_history() {
        let mut editor = transactional_editor_with_redo();
        let before = transactional_editor_state(&editor);

        assert_eq!(
            editor.execute(Command::InsertText {
                text: " rejected".to_owned(),
            }),
            Err(EditError::Model(ModelError::IdExhausted))
        );

        assert_eq!(transactional_editor_state(&editor), before);
    }

    #[test]
    fn transactional_revision_exhaustion_preserves_transaction_commit_state_and_history() {
        let mut editor = transactional_editor_with_redo();
        let before = transactional_editor_state(&editor);
        let mut transaction = editor.begin_transaction();
        transaction
            .execute(Command::InsertText {
                text: " rejected".to_owned(),
            })
            .unwrap();

        assert_eq!(
            transaction.commit(),
            Err(EditError::Model(ModelError::IdExhausted))
        );

        assert_eq!(transactional_editor_state(&editor), before);
    }

    #[test]
    fn transactional_revision_exhaustion_preserves_undo_state_and_history() {
        let mut editor = transactional_editor_with_redo();
        let before = transactional_editor_state(&editor);

        assert_eq!(
            editor.undo(),
            Err(EditError::Model(ModelError::IdExhausted))
        );

        assert_eq!(transactional_editor_state(&editor), before);
    }

    #[test]
    fn transactional_revision_exhaustion_preserves_redo_state_and_history() {
        let mut editor = transactional_editor_with_redo();
        let before = transactional_editor_state(&editor);

        assert_eq!(
            editor.redo(),
            Err(EditError::Model(ModelError::IdExhausted))
        );

        assert_eq!(transactional_editor_state(&editor), before);
    }

    #[test]
    fn transactional_command_error_preserves_staged_document_and_allows_retry() {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: "A🙂B".to_owned(),
            })
            .unwrap();
        editor.clear_history();
        editor.document.next_block = u64::MAX;
        let before = transactional_editor_state(&editor);
        let mut transaction = editor.begin_transaction();
        let staged_before = transaction.document.clone();
        let selection_before = transaction.selection();

        assert_eq!(
            transaction.execute(Command::SplitBlock),
            Err(EditError::Model(ModelError::IdExhausted))
        );
        assert_eq!(transaction.document, staged_before);
        assert_eq!(transaction.selection(), selection_before);

        transaction
            .execute(Command::ReplaceRange {
                range: TextRange::new(TextOffset::new(1), TextOffset::new(2)),
                text: "🔥".to_owned(),
            })
            .unwrap();
        transaction.commit().unwrap();
        assert_eq!(editor.snapshot().blocks[0].text(), Some("A🔥B"));
        assert_eq!(editor.revision().get(), before.2.get() + 1);
        assert_eq!(editor.undo.len(), 1);
        assert!(!editor.can_redo());
        assert!(editor.undo().unwrap());
        assert_eq!(editor.document, before.0);
        assert_eq!(editor.selection(), before.1);
        assert!(!editor.can_undo());
        assert!(editor.redo().unwrap());
        assert_eq!(editor.snapshot().blocks[0].text(), Some("A🔥B"));
        assert!(!editor.can_redo());
    }

    #[test]
    fn transactional_revision_exhaustion_keeps_no_ops_successful() {
        let mut editor = Editor::new();
        editor.revision = Revision::new(u64::MAX);
        let before = transactional_editor_state(&editor);

        assert!(!editor.undo().unwrap());
        assert!(!editor.redo().unwrap());
        editor
            .execute(Command::InsertText {
                text: String::new(),
            })
            .unwrap();
        editor.begin_transaction().commit().unwrap();

        assert_eq!(transactional_editor_state(&editor), before);
    }

    #[test]
    fn transactional_staged_selection_rolls_back_and_rejects_invalid_cursors() {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: "A🙂B".to_owned(),
            })
            .unwrap();
        editor.clear_history();
        let before = transactional_editor_state(&editor);
        let container = TextContainerId::Block(BlockId::new(1));
        let selected = Selection::new(
            Cursor::new(container, TextOffset::new(1)),
            Cursor::new(container, TextOffset::new(2)),
        );
        let mut transaction = editor.begin_transaction();
        transaction.set_selection(selected).unwrap();
        assert_eq!(transaction.selection(), selected);
        assert_eq!(transactional_editor_state(transaction.editor), before);
        assert!(
            transaction
                .set_selection(Selection::collapsed(Cursor::new(
                    container,
                    TextOffset::new(4),
                )))
                .is_err()
        );
        assert_eq!(transaction.selection(), selected);
        transaction.rollback();
        assert_eq!(transactional_editor_state(&editor), before);
    }

    #[test]
    fn transactional_selected_unicode_replacement_is_one_undo_step() {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: "A🙂B".to_owned(),
            })
            .unwrap();
        editor.clear_history();
        let before = transactional_editor_state(&editor);
        let container = TextContainerId::Block(BlockId::new(1));
        let mut transaction = editor.begin_transaction();
        transaction
            .set_selection(Selection::new(
                Cursor::new(container, TextOffset::new(1)),
                Cursor::new(container, TextOffset::new(2)),
            ))
            .unwrap();
        transaction
            .execute(Command::ReplaceRange {
                range: TextRange::new(TextOffset::new(1), TextOffset::new(2)),
                text: "🔥".to_owned(),
            })
            .unwrap();
        transaction.commit().unwrap();
        assert_eq!(editor.snapshot().blocks[0].text(), Some("A🔥B"));
        assert_eq!(editor.revision().get(), before.2.get() + 1);
        assert_eq!(editor.undo.len(), 1);
        assert!(editor.undo().unwrap());
        assert_eq!(editor.document, before.0);
        assert_eq!(editor.selection(), before.1);
        assert!(!editor.can_undo());
        assert!(editor.redo().unwrap());
        assert_eq!(editor.snapshot().blocks[0].text(), Some("A🔥B"));
        assert_eq!(
            editor.selection(),
            Selection::collapsed(Cursor::new(container, TextOffset::new(2)))
        );
        assert!(!editor.can_redo());
    }

    #[test]
    fn clear_history_discards_undo_and_redo_without_changing_editor_state() {
        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: "first".to_string(),
            })
            .unwrap();
        editor
            .execute(Command::InsertText {
                text: " second".to_string(),
            })
            .unwrap();
        assert!(editor.undo().unwrap());
        assert!(editor.can_undo());
        assert!(editor.can_redo());

        let document_before = editor.snapshot();
        let selection_before = editor.selection();
        let revision_before = editor.revision();
        editor.clear_history();

        assert_eq!(editor.snapshot().blocks, document_before.blocks);
        assert_eq!(editor.selection(), selection_before);
        assert_eq!(editor.revision(), revision_before);
        assert!(!editor.can_undo());
        assert!(!editor.can_redo());
    }

    #[test]
    fn replace_document_skips_leading_image_block() {
        let image_id = ImageId::new(1);
        let text_id = BlockId::new(2);
        let snapshot = DocumentSnapshot {
            revision: Revision::new(7),
            blocks: vec![
                BlockSnapshot::Image(ImageSnapshot {
                    id: image_id,
                    payload: ImagePayload::new(
                        ImageFormat::Png,
                        PixelSize::new(1, 1).unwrap(),
                        vec![0, 1, 2],
                    )
                    .unwrap(),
                    layout: ImageLayout::default(),
                }),
                BlockSnapshot::Text(TextBlockSnapshot {
                    id: text_id,
                    kind: BlockKind::Paragraph,
                    paragraph_style: ParagraphStyle::default(),
                    text: "after image".to_string(),
                    runs: vec![InlineRun::new("after image", TextStyle::default())],
                }),
            ],
        };

        let mut editor = Editor::new();
        let before = editor.snapshot();
        let before_revision = editor.revision();

        let result = editor.replace_document(snapshot.clone());

        assert_eq!(
            result,
            Ok(()),
            "expected replace_document to accept a snapshot whose first block is an image"
        );
        assert_eq!(
            editor.revision(),
            Revision::new(before_revision.get() + 1),
            "replace_document should advance revision after a successful commit"
        );
        assert_eq!(
            editor.snapshot().revision,
            Revision::new(before_revision.get() + 1),
            "editor snapshot revision should match the updated revision"
        );
        assert_eq!(
            editor.snapshot().blocks.len(),
            2,
            "replace_document should preserve all blocks, including the leading image"
        );
        assert_eq!(
            editor.snapshot().blocks[0].id(),
            image_id.get(),
            "first block should remain the image"
        );
        assert_eq!(
            editor.selection(),
            Selection::collapsed(Cursor::new(
                TextContainerId::Block(text_id),
                TextOffset::new(0)
            )),
            "selection should land at the first available text block"
        );
        assert!(
            editor.can_undo(),
            "successful replace_document should create an undo entry"
        );
        let undo_result = editor.undo();
        assert_eq!(undo_result, Ok(true), "undo should succeed");
        assert_eq!(
            editor.snapshot().blocks,
            before.blocks,
            "undo should restore the previous blocks"
        );
        assert_eq!(
            editor.revision(),
            Revision::new(before_revision.get() + 2),
            "undo should advance revision"
        );
    }

    #[test]
    fn replace_document_table_after_leading_image() {
        let table_id = TableId::new(3);
        let cell_id = CellId::new(4);
        let snapshot = DocumentSnapshot {
            revision: Revision::new(1),
            blocks: vec![
                BlockSnapshot::Image(ImageSnapshot {
                    id: ImageId::new(10),
                    payload: ImagePayload::new(
                        ImageFormat::Jpeg,
                        PixelSize::new(2, 2).unwrap(),
                        vec![1, 2, 3],
                    )
                    .unwrap(),
                    layout: ImageLayout::default(),
                }),
                BlockSnapshot::Table(TableSnapshot {
                    id: table_id,
                    columns: vec![TableColumnSnapshot {
                        id: ColumnId::new(1),
                        width_half_points: 720,
                    }],
                    rows: vec![TableRowSnapshot {
                        id: RowId::new(1),
                        cells: vec![TableCellSnapshot {
                            id: cell_id,
                            text: "cell text".to_string(),
                            runs: vec![InlineRun::new("cell text", TextStyle::default())],
                            style: CellStyle::default(),
                            alignment: CellAlignment::default(),
                        }],
                    }],
                }),
            ],
        };

        let mut editor = Editor::new();

        let result = editor.replace_document(snapshot);

        assert_eq!(result, Ok(()));
        assert_eq!(
            editor.selection(),
            Selection::collapsed(Cursor::new(
                TextContainerId::Cell(cell_id),
                TextOffset::new(0)
            )),
            "selection should land at the first available cell when image precedes the only table"
        );
    }

    #[test]
    fn replace_document_image_only_snapshot_returns_invalid_selection() {
        let snapshot = DocumentSnapshot {
            revision: Revision::new(2),
            blocks: vec![BlockSnapshot::Image(ImageSnapshot {
                id: ImageId::new(20),
                payload: ImagePayload::new(
                    ImageFormat::WebP,
                    PixelSize::new(4, 4).unwrap(),
                    vec![7, 8, 9],
                )
                .unwrap(),
                layout: ImageLayout::default(),
            })],
        };

        let mut editor = Editor::new();
        let before = editor.snapshot();

        let result = editor.replace_document(snapshot);

        assert_eq!(result, Err(EditError::InvalidSelection));
        assert_eq!(
            editor.snapshot(),
            before,
            "editor should not mutate on invalid selection"
        );
        assert_eq!(
            editor.revision(),
            before.revision,
            "revision should stay unchanged"
        );
        assert_eq!(
            editor.selection(),
            Selection::collapsed(Cursor::new(
                TextContainerId::Block(BlockId::new(1)),
                TextOffset::new(0)
            )),
            "previous selection should be preserved"
        );
    }

    #[test]
    fn replace_document_preserves_valid_snapshot_starting_with_text() {
        let text_id = BlockId::new(5);
        let snapshot = DocumentSnapshot {
            revision: Revision::new(3),
            blocks: vec![BlockSnapshot::Text(TextBlockSnapshot {
                id: text_id,
                kind: BlockKind::Paragraph,
                paragraph_style: ParagraphStyle::default(),
                text: "first text".to_string(),
                runs: vec![InlineRun::new("first text", TextStyle::default())],
            })],
        };

        let mut editor = Editor::new();
        editor
            .execute(Command::InsertText {
                text: "existing".to_string(),
            })
            .unwrap();

        let result = editor.replace_document(snapshot);

        assert_eq!(result, Ok(()));
        assert_eq!(editor.snapshot().blocks.len(), 1);
        assert_eq!(editor.snapshot().blocks[0].text(), Some("first text"));
        assert_eq!(
            editor.selection(),
            Selection::collapsed(Cursor::new(
                TextContainerId::Block(text_id),
                TextOffset::new(0)
            ))
        );
    }
}
