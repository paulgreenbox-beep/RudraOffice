//! Application-owned module extracted without changing behavior.
use super::*;

#[derive(Debug)]
pub struct RenderedPage {
    pub page_index: PageId,
    pub scale: f32,
    pub png: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct CacheEntry {
    pub(crate) key: (DocumentId, PageId, u32),
    png: Vec<u8>,
}

/// Maximum decoded/rendered bytes retained by the unified PDF viewer cache.
pub(crate) const PDF_VIEW_CACHE_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub struct PdfPageCache {
    pub(crate) entries: VecDeque<CacheEntry>,
    capacity: usize,
    bytes: usize,
    max_bytes: usize,
}

impl Default for PdfPageCache {
    fn default() -> Self {
        Self::new(24)
    }
}

impl PdfPageCache {
    pub fn new(capacity: usize) -> Self {
        Self::with_limits(capacity, PDF_VIEW_CACHE_MAX_BYTES)
    }

    pub fn with_limits(capacity: usize, max_bytes: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::with_capacity(capacity.max(1)),
            bytes: 0,
            max_bytes: max_bytes.max(1),
        }
    }

    pub fn get(
        &mut self,
        document_id: DocumentId,
        page_index: PageId,
        scale: f32,
    ) -> Option<&[u8]> {
        let key = (document_id, page_index, scale.to_bits());
        let position = self.entries.iter().position(|entry| entry.key == key)?;
        let entry = self.entries.remove(position).expect("position exists");
        self.entries.push_front(entry);
        self.entries.front().map(|entry| entry.png.as_slice())
    }

    pub fn insert(
        &mut self,
        document_id: DocumentId,
        page_index: PageId,
        scale: f32,
        png: Vec<u8>,
    ) {
        let key = (document_id, page_index, scale.to_bits());
        if let Some(position) = self.entries.iter().position(|entry| entry.key == key)
            && let Some(entry) = self.entries.remove(position)
        {
            self.bytes = self.bytes.saturating_sub(entry.png.len());
        }
        self.bytes = self.bytes.saturating_add(png.len());
        self.entries.push_front(CacheEntry { key, png });
        self.evict();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn remove_document(&mut self, document_id: DocumentId) {
        self.entries.retain(|entry| entry.key.0 != document_id);
        self.bytes = self.entries.iter().map(|entry| entry.png.len()).sum();
    }
    fn evict(&mut self) {
        while self.entries.len() > self.capacity || self.bytes > self.max_bytes {
            let Some(entry) = self.entries.pop_back() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(entry.png.len());
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PdfViewStatus {
    Empty,
    Loading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum PdfViewEffect {
    #[default]
    None,
    RenderPage {
        page_index: PageId,
        scale: f32,
    },
    ShowLoading,
    ShowError(String),
    ShowEmpty,
}

pub(crate) const PDF_MIN_SCALE: f32 = 0.25;
pub(crate) const PDF_MAX_SCALE: f32 = 4.0;
pub(crate) const PDF_FIT_VIEWPORT_WIDTH_POINTS: f32 = 760.0;
pub(crate) const PDF_FIT_VIEWPORT_HEIGHT_POINTS: f32 = 680.0;
pub(crate) const PDF_PAGE_GAP_POINTS: f32 = 24.0;
pub(crate) const PDF_VIEW_PADDING_POINTS: f32 = 24.0;
pub(crate) const PDF_MAX_VISIBLE_PAGES: usize = 8;
pub(crate) const PDF_SCROLL_STEP_POINTS: f32 = 120.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfZoomMode {
    Custom,
    FitWidth,
    FitPage,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfPagePlacement {
    pub page_index: PageId,
    pub page_number: u32,
    pub geometry: PageGeometry,
    pub x_points: f32,
    pub y_points: f32,
    pub width_points: f32,
    pub height_points: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfFramePage {
    pub placement: PdfPagePlacement,
    pub png: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfViewerFrame {
    /// Backend identity owning the raster/texture; PageId alone is not global.
    pub document_id: Option<DocumentId>,
    pub status: PdfViewStatus,
    pub error: Option<String>,
    pub current_page: PageId,
    pub current_page_number: u32,
    pub page_count: u32,
    pub scale: f32,
    pub zoom_mode: PdfZoomMode,
    pub viewport_width_points: f32,
    pub viewport_height_points: f32,
    pub content_width_points: f32,
    pub content_height_points: f32,
    pub scroll_x_points: f32,
    pub scroll_y_points: f32,
    pub pages: Vec<PdfFramePage>,
}

#[derive(Debug)]
pub struct PdfViewState {
    pub status: PdfViewStatus,
    pub error: Option<String>,
    pub current_page: PageId,
    pub scale: f32,
    pub zoom_mode: PdfZoomMode,
    pub page_count: u32,
    pub document_id: Option<DocumentId>,
    pub page_ids: Vec<PageId>,
    pub page_geometries: Vec<PageGeometry>,
    pub visible_pages: Vec<RenderedPage>,
    pub png: Vec<u8>,
    pub viewport_width_points: f32,
    pub viewport_height_points: f32,
    pub content_width_points: f32,
    pub content_height_points: f32,
    pub scroll_x_points: f32,
    pub scroll_y_points: f32,
}

impl Default for PdfViewState {
    fn default() -> Self {
        Self {
            status: PdfViewStatus::Empty,
            error: None,
            current_page: PageId::new(1),
            scale: 1.0,
            zoom_mode: PdfZoomMode::Custom,
            page_count: 0,
            document_id: None,
            page_ids: Vec::new(),
            page_geometries: Vec::new(),
            visible_pages: Vec::new(),
            png: Vec::new(),
            viewport_width_points: PDF_FIT_VIEWPORT_WIDTH_POINTS,
            viewport_height_points: PDF_FIT_VIEWPORT_HEIGHT_POINTS,
            content_width_points: PDF_FIT_VIEWPORT_WIDTH_POINTS,
            content_height_points: PDF_FIT_VIEWPORT_HEIGHT_POINTS,
            scroll_x_points: 0.0,
            scroll_y_points: 0.0,
        }
    }
}

impl PdfViewState {
    pub fn bind_pdf(&mut self, document_id: DocumentId, page_count: u32, page_ids: &[PageId]) {
        self.bind_pdf_with_geometries(document_id, page_count, page_ids, &[]);
    }

    pub fn bind_pdf_with_geometries(
        &mut self,
        document_id: DocumentId,
        page_count: u32,
        page_ids: &[PageId],
        geometries: &[PageGeometry],
    ) {
        self.document_id = Some(document_id);
        self.page_count = page_count;
        self.page_ids = page_ids.to_vec();
        self.page_geometries = page_ids
            .iter()
            .enumerate()
            .map(|(position, _)| {
                geometries
                    .get(position)
                    .copied()
                    .unwrap_or_else(PageGeometry::letter)
            })
            .collect();
        self.current_page = self.first_page_id().unwrap_or(PageId::new(1));
        self.scale = 1.0;
        self.zoom_mode = PdfZoomMode::Custom;
        self.visible_pages.clear();
        self.png.clear();
        self.error = None;
        self.status = PdfViewStatus::Ready;
        self.scroll_x_points = 0.0;
        self.scroll_y_points = 0.0;
        self.recompute_content_extent();
        self.scroll_to_page(self.current_page);
    }

    pub fn clear(&mut self) {
        self.document_id = None;
        self.page_count = 0;
        self.page_ids.clear();
        self.page_geometries.clear();
        self.current_page = PageId::new(1);
        self.scale = 1.0;
        self.zoom_mode = PdfZoomMode::Custom;
        self.visible_pages.clear();
        self.png.clear();
        self.error = None;
        self.status = PdfViewStatus::Empty;
        self.content_width_points = self.viewport_width_points;
        self.content_height_points = self.viewport_height_points;
        self.scroll_x_points = 0.0;
        self.scroll_y_points = 0.0;
    }

    pub fn first_page_id(&self) -> Option<PageId> {
        self.page_ids.first().copied()
    }

    pub fn last_page_id(&self) -> Option<PageId> {
        self.page_ids.last().copied()
    }

    pub fn page_ids(&self) -> &[PageId] {
        &self.page_ids
    }

    pub fn page_placements(&self) -> Vec<PdfPagePlacement> {
        let max_width = self
            .page_geometries
            .iter()
            .map(|geometry| geometry.width_points.max(1.0) * self.scale)
            .fold(1.0_f32, |current, width| current.max(width));
        let mut y_points = PDF_VIEW_PADDING_POINTS;
        self.page_ids
            .iter()
            .enumerate()
            .map(|(position, page_index)| {
                let geometry = self
                    .page_geometries
                    .get(position)
                    .copied()
                    .unwrap_or_else(PageGeometry::letter);
                let width_points = geometry.width_points.max(1.0) * self.scale;
                let height_points = geometry.height_points.max(1.0) * self.scale;
                let placement = PdfPagePlacement {
                    page_index: *page_index,
                    page_number: position as u32 + 1,
                    geometry,
                    x_points: PDF_VIEW_PADDING_POINTS + (max_width - width_points) * 0.5,
                    y_points,
                    width_points,
                    height_points,
                };
                y_points += height_points + PDF_PAGE_GAP_POINTS;
                placement
            })
            .collect()
    }

    pub fn recompute_content_extent(&mut self) {
        let placements = self.page_placements();
        self.content_width_points = placements
            .iter()
            .map(|placement| placement.x_points + placement.width_points)
            .fold(self.viewport_width_points, f32::max)
            + PDF_VIEW_PADDING_POINTS;
        self.content_height_points = placements
            .iter()
            .map(|placement| placement.y_points + placement.height_points)
            .fold(self.viewport_height_points, f32::max)
            + PDF_VIEW_PADDING_POINTS;
        self.scroll_x_points = self.scroll_x_points.clamp(0.0, self.max_scroll_x());
        self.scroll_y_points = self.scroll_y_points.clamp(0.0, self.max_scroll_y());
    }

    pub fn max_scroll_x(&self) -> f32 {
        (self.content_width_points - self.viewport_width_points).max(0.0)
    }

    pub fn max_scroll_y(&self) -> f32 {
        (self.content_height_points - self.viewport_height_points).max(0.0)
    }

    pub fn set_viewport_points(&mut self, width_points: f32, height_points: f32) {
        self.viewport_width_points = width_points.max(1.0);
        self.viewport_height_points = height_points.max(1.0);
        self.recompute_content_extent();
    }

    pub fn visible_page_ids(&self) -> Vec<PageId> {
        let top = self.scroll_y_points - self.viewport_height_points;
        let bottom = self.scroll_y_points + self.viewport_height_points * 2.0;
        let mut candidates = self
            .page_placements()
            .into_iter()
            .filter(|placement| {
                placement.y_points + placement.height_points >= top && placement.y_points <= bottom
            })
            .map(|placement| placement.page_index)
            .collect::<Vec<_>>();
        if !candidates.contains(&self.current_page) {
            candidates.push(self.current_page);
        }
        candidates.sort_by_key(|page_id| self.page_index_position(*page_id).unwrap_or(usize::MAX));
        candidates.dedup();
        if candidates.len() <= PDF_MAX_VISIBLE_PAGES {
            return candidates;
        }
        let current_position = self.page_index_position(self.current_page).unwrap_or(0);
        let start = current_position
            .saturating_sub(PDF_MAX_VISIBLE_PAGES / 2)
            .min(self.page_ids.len().saturating_sub(PDF_MAX_VISIBLE_PAGES));
        self.page_ids
            .iter()
            .skip(start)
            .take(PDF_MAX_VISIBLE_PAGES)
            .copied()
            .collect()
    }

    pub fn scroll_to_page(&mut self, page_index: PageId) {
        let Some(placement) = self
            .page_placements()
            .into_iter()
            .find(|placement| placement.page_index == page_index)
        else {
            return;
        };
        self.scroll_y_points = if placement.height_points <= self.viewport_height_points {
            placement.y_points - (self.viewport_height_points - placement.height_points) * 0.5
        } else {
            placement.y_points
        }
        .clamp(0.0, self.max_scroll_y());
        self.scroll_x_points = if placement.width_points <= self.viewport_width_points {
            placement.x_points - (self.viewport_width_points - placement.width_points) * 0.5
        } else {
            placement.x_points
        }
        .clamp(0.0, self.max_scroll_x());
    }

    pub fn update_current_page_from_scroll(&mut self) -> bool {
        let center = self.scroll_y_points + self.viewport_height_points * 0.5;
        let Some(page_index) = self
            .page_placements()
            .into_iter()
            .min_by(|left, right| {
                let left_distance = (left.y_points + left.height_points * 0.5 - center).abs();
                let right_distance = (right.y_points + right.height_points * 0.5 - center).abs();
                left_distance
                    .partial_cmp(&right_distance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|placement| placement.page_index)
        else {
            return false;
        };
        if self.current_page == page_index {
            false
        } else {
            self.current_page = page_index;
            self.error = None;
            true
        }
    }

    /// Restores an absolute logical scroll offset after geometry has been
    /// bound. This differs from wheel/key input, which is relative.
    pub fn set_scroll_offset_points(&mut self, scroll_x: f32, scroll_y: f32) -> bool {
        let next_x = scroll_x.clamp(0.0, self.max_scroll_x());
        let next_y = scroll_y.clamp(0.0, self.max_scroll_y());
        let changed = (next_x - self.scroll_x_points).abs() > f32::EPSILON
            || (next_y - self.scroll_y_points).abs() > f32::EPSILON;
        self.scroll_x_points = next_x;
        self.scroll_y_points = next_y;
        self.update_current_page_from_scroll() || changed
    }

    pub fn scroll_by_points(&mut self, delta_x: f32, delta_y: f32) -> bool {
        self.set_scroll_offset_points(
            self.scroll_x_points + delta_x,
            self.scroll_y_points + delta_y,
        )
    }

    pub fn page_index_position(&self, page_index: PageId) -> Option<usize> {
        self.page_ids.iter().position(|id| *id == page_index)
    }

    pub fn geometry_for(&self, page_index: PageId) -> PageGeometry {
        self.page_index_position(page_index)
            .and_then(|position| self.page_geometries.get(position).copied())
            .unwrap_or_else(PageGeometry::letter)
    }

    pub fn can_previous(&self) -> bool {
        self.page_index_position(self.current_page)
            .map(|pos| pos > 0)
            .unwrap_or(false)
    }

    pub fn can_next(&self) -> bool {
        self.page_index_position(self.current_page)
            .map(|pos| pos + 1 < self.page_ids.len())
            .unwrap_or(false)
    }

    pub fn current_page_number(&self) -> u32 {
        self.page_index_position(self.current_page)
            .map(|pos| pos as u32 + 1)
            .unwrap_or(0)
    }

    pub fn previous_page_id(&self) -> Option<PageId> {
        let pos = self.page_index_position(self.current_page)?;
        pos.checked_sub(1)
            .and_then(|prev| self.page_ids.get(prev).copied())
    }

    pub fn next_page_id(&self) -> Option<PageId> {
        let pos = self.page_index_position(self.current_page)?;
        self.page_ids.get(pos + 1).copied()
    }

    pub fn zoom_in(&mut self, step: f32) {
        self.set_scale(self.scale + step);
    }

    pub fn zoom_out(&mut self, step: f32) {
        self.set_scale(self.scale - step);
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale.clamp(PDF_MIN_SCALE, PDF_MAX_SCALE);
        self.zoom_mode = PdfZoomMode::Custom;
        self.recompute_content_extent();
        self.scroll_to_page(self.current_page);
    }

    pub fn fit_width(&mut self, viewport_width_points: f32) {
        self.scale = compute_pdf_fit_width_scale(
            self.geometry_for(self.current_page),
            viewport_width_points,
        );
        self.zoom_mode = PdfZoomMode::FitWidth;
        self.recompute_content_extent();
        self.scroll_to_page(self.current_page);
    }

    pub fn fit_page(&mut self, viewport_width_points: f32, viewport_height_points: f32) {
        self.scale = compute_pdf_fit_page_scale(
            self.geometry_for(self.current_page),
            viewport_width_points,
            viewport_height_points,
        );
        self.zoom_mode = PdfZoomMode::FitPage;
        self.recompute_content_extent();
        self.scroll_to_page(self.current_page);
    }

    pub fn set_page(&mut self, page_index: PageId) -> bool {
        if page_index.value() == 0 || !self.page_ids.contains(&page_index) {
            return false;
        }
        if self.current_page == page_index {
            return true;
        }
        self.current_page = page_index;
        self.scroll_to_page(page_index);
        self.visible_pages.clear();
        self.png.clear();
        self.error = None;
        self.status = PdfViewStatus::Ready;
        true
    }

    pub fn go_previous(&mut self) -> bool {
        self.previous_page_id().is_some_and(|id| self.set_page(id))
    }

    pub fn go_next(&mut self) -> bool {
        self.next_page_id().is_some_and(|id| self.set_page(id))
    }

    pub fn go_first(&mut self) -> bool {
        self.first_page_id().is_some_and(|id| self.set_page(id))
    }

    pub fn go_last(&mut self) -> bool {
        self.last_page_id().is_some_and(|id| self.set_page(id))
    }
}

pub(crate) fn restore_pdf_session_view(state: &mut PdfViewState, view: PdfSessionView) {
    state.set_scale(view.zoom);
    if view.initialized {
        let _ = state.set_scroll_offset_points(view.scroll_x, view.scroll_y);
    } else {
        // The controller has the first reliable geometry at activation time.
        // Re-centre the current page after zoom instead of restoring the
        // placeholder zero scroll that a newly created session carries.
        state.scroll_to_page(state.current_page);
    }
}

pub fn compute_pdf_fit_width_scale(geometry: PageGeometry, viewport_width_points: f32) -> f32 {
    let width = viewport_width_points.max(1.0);
    (width / geometry.width_points.max(1.0)).clamp(PDF_MIN_SCALE, PDF_MAX_SCALE)
}

pub fn compute_pdf_fit_page_scale(
    geometry: PageGeometry,
    viewport_width_points: f32,
    viewport_height_points: f32,
) -> f32 {
    let width_scale = viewport_width_points.max(1.0) / geometry.width_points.max(1.0);
    let height_scale = viewport_height_points.max(1.0) / geometry.height_points.max(1.0);
    width_scale
        .min(height_scale)
        .clamp(PDF_MIN_SCALE, PDF_MAX_SCALE)
}

#[derive(Debug)]
pub struct PdfController {
    state: PdfViewState,
    pub(crate) cache: PdfPageCache,
}

impl Default for PdfController {
    fn default() -> Self {
        Self {
            state: PdfViewState::default(),
            cache: PdfPageCache::new(24),
        }
    }
}

impl PdfController {
    pub fn state(&self) -> &PdfViewState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut PdfViewState {
        &mut self.state
    }

    pub fn bind_document(
        &mut self,
        document_id: DocumentId,
        page_count: u32,
        page_ids: &[PageId],
    ) -> PdfViewEffect {
        self.bind_document_with_geometries(document_id, page_count, page_ids, &[])
    }

    pub fn bind_document_with_geometries(
        &mut self,
        document_id: DocumentId,
        page_count: u32,
        page_ids: &[PageId],
        geometries: &[PageGeometry],
    ) -> PdfViewEffect {
        self.state
            .bind_pdf_with_geometries(document_id, page_count, page_ids, geometries);
        self.request_current_page()
    }

    pub fn clear(&mut self) -> PdfViewEffect {
        self.state.clear();
        PdfViewEffect::ShowEmpty
    }
    /// Releases cache and view bindings for one backend PDF document.
    pub fn release_document(&mut self, document_id: DocumentId) {
        self.cache.remove_document(document_id);
        if self.state.document_id == Some(document_id) {
            self.state.clear();
        }
    }

    pub fn previous(&mut self) -> PdfViewEffect {
        if self.state.go_previous() {
            self.request_current_page()
        } else {
            PdfViewEffect::None
        }
    }

    pub fn next_page(&mut self) -> PdfViewEffect {
        if self.state.go_next() {
            self.request_current_page()
        } else {
            PdfViewEffect::None
        }
    }

    pub fn first_page(&mut self) -> PdfViewEffect {
        if self.state.go_first() {
            self.request_current_page()
        } else {
            PdfViewEffect::None
        }
    }

    pub fn last_page(&mut self) -> PdfViewEffect {
        if self.state.go_last() {
            self.request_current_page()
        } else {
            PdfViewEffect::None
        }
    }

    pub fn go_to_page_number(&mut self, page_number: u32) -> PdfViewEffect {
        let Some(page_id) = page_number
            .checked_sub(1)
            .and_then(|position| self.state.page_ids.get(position as usize).copied())
        else {
            return PdfViewEffect::None;
        };
        if self.state.set_page(page_id) {
            self.request_current_page()
        } else {
            PdfViewEffect::None
        }
    }

    pub fn zoom_in(&mut self) -> PdfViewEffect {
        self.state.zoom_in(0.25);
        self.request_current_page()
    }

    pub fn zoom_out(&mut self) -> PdfViewEffect {
        self.state.zoom_out(0.25);
        self.request_current_page()
    }

    pub fn reset_zoom(&mut self) -> PdfViewEffect {
        self.state.set_scale(1.0);
        self.request_current_page()
    }

    pub fn fit_width(&mut self) -> PdfViewEffect {
        self.state.fit_width(self.state.viewport_width_points);
        self.request_current_page()
    }

    pub fn fit_page(&mut self) -> PdfViewEffect {
        self.state.fit_page(
            self.state.viewport_width_points,
            self.state.viewport_height_points,
        );
        self.request_current_page()
    }

    pub fn set_viewport_points(&mut self, width_points: f32, height_points: f32) -> PdfViewEffect {
        let width_points = width_points.max(1.0);
        let height_points = height_points.max(1.0);
        if (self.state.viewport_width_points - width_points).abs() < 0.5
            && (self.state.viewport_height_points - height_points).abs() < 0.5
        {
            return PdfViewEffect::None;
        }
        let zoom_mode = self.state.zoom_mode;
        self.state.set_viewport_points(width_points, height_points);
        match zoom_mode {
            PdfZoomMode::FitWidth => self.state.fit_width(width_points),
            PdfZoomMode::FitPage => self.state.fit_page(width_points, height_points),
            PdfZoomMode::Custom => {}
        }
        self.request_current_page()
    }

    pub fn scroll_by(&mut self, delta_x: f32, delta_y: f32) -> PdfViewEffect {
        if self.state.scroll_by_points(delta_x, delta_y) {
            self.request_current_page()
        } else {
            PdfViewEffect::None
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> PdfViewEffect {
        match key {
            KeyCode::ArrowUp => self.scroll_by(0.0, -PDF_SCROLL_STEP_POINTS),
            KeyCode::ArrowDown => self.scroll_by(0.0, PDF_SCROLL_STEP_POINTS),
            KeyCode::ArrowLeft => self.scroll_by(-PDF_SCROLL_STEP_POINTS, 0.0),
            KeyCode::ArrowRight => self.scroll_by(PDF_SCROLL_STEP_POINTS, 0.0),
            KeyCode::PageUp => self.scroll_by(0.0, -self.state.viewport_height_points),
            KeyCode::PageDown => self.scroll_by(0.0, self.state.viewport_height_points),
            KeyCode::Home => {
                if self.state.go_first() {
                    self.request_current_page()
                } else {
                    PdfViewEffect::None
                }
            }
            KeyCode::End => {
                if self.state.go_last() {
                    self.request_current_page()
                } else {
                    PdfViewEffect::None
                }
            }
            _ => PdfViewEffect::None,
        }
    }

    pub fn apply_effect(&mut self, effect: PdfViewEffect, engine: &mut PdfOxideAdapter) {
        match effect {
            PdfViewEffect::None => {}
            PdfViewEffect::ShowLoading => {
                self.state.status = PdfViewStatus::Loading;
                self.state.error = None;
            }
            PdfViewEffect::ShowError(error) => {
                self.state.status = PdfViewStatus::Error;
                self.state.error = Some(error);
                self.state.visible_pages.clear();
                self.state.png.clear();
            }
            PdfViewEffect::ShowEmpty => {
                self.state.status = PdfViewStatus::Empty;
                self.state.error = None;
                self.state.visible_pages.clear();
                self.state.png.clear();
            }
            PdfViewEffect::RenderPage { page_index, scale } => {
                if !self.state.set_page(page_index) {
                    self.state.status = PdfViewStatus::Error;
                    self.state.error = Some("page index is invalid".into());
                    self.state.visible_pages.clear();
                    self.state.png.clear();
                    return;
                }
                self.state.scale = scale;
                self.state.recompute_content_extent();
                let Some(document_id) = self.state.document_id else {
                    self.state.status = PdfViewStatus::Error;
                    self.state.error = Some("no bound pdf document".into());
                    self.state.visible_pages.clear();
                    self.state.png.clear();
                    return;
                };
                let current_page = self.state.current_page;
                let page_ids = self.state.page_ids.clone();
                if self.state.page_index_position(current_page).is_none() {
                    self.state.status = PdfViewStatus::Error;
                    self.state.error = Some("current page is not bound".into());
                    self.state.visible_pages.clear();
                    self.state.png.clear();
                    return;
                }
                let render_ids = self.state.visible_page_ids();
                let mut visible_pages = Vec::with_capacity(render_ids.len());
                let mut current_png = None;
                let mut current_error = None;
                for page_id in render_ids {
                    match render_page_from_cache(
                        &mut self.cache,
                        engine,
                        document_id,
                        page_id,
                        scale,
                        &page_ids,
                    ) {
                        Ok(png) => {
                            if page_id == current_page {
                                current_png = Some(png.clone());
                            }
                            visible_pages.push(RenderedPage {
                                page_index: page_id,
                                scale,
                                png,
                            });
                        }
                        Err(error) if page_id == current_page => {
                            current_error = Some(error);
                        }
                        Err(_) => {}
                    }
                }
                if let Some(png) = current_png {
                    self.state.status = PdfViewStatus::Ready;
                    self.state.error = None;
                    self.state.png = png;
                    self.state.visible_pages = visible_pages;
                } else {
                    self.state.status = PdfViewStatus::Error;
                    self.state.error =
                        current_error.or_else(|| Some("current PDF page did not render".into()));
                    self.state.visible_pages.clear();
                    self.state.png.clear();
                }
            }
        }
    }

    pub(crate) fn request_current_page(&self) -> PdfViewEffect {
        if self.state.status == PdfViewStatus::Empty {
            return PdfViewEffect::None;
        }
        if self.state.page_ids.is_empty() {
            return PdfViewEffect::ShowEmpty;
        }
        if self.state.status == PdfViewStatus::Error && self.state.document_id.is_none() {
            return PdfViewEffect::ShowEmpty;
        }
        let Some(_document_id) = self.state.document_id else {
            return PdfViewEffect::ShowEmpty;
        };
        PdfViewEffect::RenderPage {
            page_index: self.state.current_page,
            scale: self.state.scale,
        }
    }
}

pub(crate) fn render_page_from_cache(
    cache: &mut PdfPageCache,
    engine: &mut PdfOxideAdapter,
    document_id: DocumentId,
    page_index: PageId,
    scale: f32,
    page_ids: &[PageId],
) -> Result<Vec<u8>, String> {
    if page_index.value() == 0 {
        return Err("PageId(0) невалиден.".to_owned());
    }
    if !page_ids.contains(&page_index) {
        return Err("page_index вне диапазона page_ids".to_owned());
    }
    if !scale.is_finite() || !(PDF_MIN_SCALE..=PDF_MAX_SCALE).contains(&scale) {
        return Err("scale должен быть конечным и находиться в диапазоне [0.01, 10.0]".to_owned());
    }
    if let Some(png) = cache.get(document_id, page_index, scale) {
        return Ok(png.to_vec());
    }

    let png = PdfEngine::render_page(engine, document_id, page_index, scale)
        .map_err(|error| error.to_string())?;
    cache.insert(document_id, page_index, scale, png.clone());
    Ok(png)
}

impl AppState {
    pub(crate) fn pdf_active(&self) -> bool {
        matches!(self.document(), Some(OpenDocument::Pdf { .. }))
    }

    pub(crate) fn should_show_pdf_viewer(&self) -> bool {
        self.active_module() == Module::Pdf && self.pdf_active()
    }

    pub(crate) fn apply_pdf_effect(&mut self) {
        if matches!(self.pdf_effect, PdfViewEffect::None) {
            return;
        }
        let effect = self.pdf_effect.clone();
        self.pdf_effect = PdfViewEffect::None;
        if matches!(effect, PdfViewEffect::RenderPage { .. }) {
            self.pdf_loading = true;
            self.pdf_error = None;
        }
        self.pdf_controller
            .apply_effect(effect, &mut self.pdf_engine);
        let controller_status = self.pdf_controller.state().status;
        match controller_status {
            PdfViewStatus::Empty | PdfViewStatus::Ready => {
                self.pdf_loading = false;
                self.pdf_error = None;
            }
            PdfViewStatus::Loading => {
                self.pdf_loading = true;
                self.pdf_error = None;
            }
            PdfViewStatus::Error => {
                self.pdf_loading = false;
                self.pdf_error = self.pdf_controller.state().error.clone();
            }
        }
        self.sync_active_session_view();
    }

    pub(crate) fn bind_pdf_document(
        &mut self,
        document_id: DocumentId,
        page_count: u32,
        page_ids: &[PageId],
    ) {
        let geometries = page_ids
            .iter()
            .map(|page_id| {
                PdfEngine::page_geometry(&mut self.pdf_engine, document_id, *page_id)
                    .unwrap_or_else(|_| PageGeometry::letter())
            })
            .collect::<Vec<_>>();
        self.pdf_effect = self.pdf_controller.bind_document_with_geometries(
            document_id,
            page_count,
            page_ids,
            &geometries,
        );
        self.apply_pdf_effect();
    }

    pub(crate) fn clear_pdf_document(&mut self) {
        self.pdf_effect = self.pdf_controller.clear();
        self.apply_pdf_effect();
    }

    pub(crate) fn pdf_previous(&mut self) {
        self.pdf_effect = self.pdf_controller.previous();
        self.apply_pdf_effect();
    }

    pub(crate) fn pdf_next(&mut self) {
        self.pdf_effect = self.pdf_controller.next_page();
        self.apply_pdf_effect();
    }

    pub(crate) fn pdf_first(&mut self) {
        self.pdf_effect = self.pdf_controller.first_page();
        self.apply_pdf_effect();
    }

    pub(crate) fn pdf_last(&mut self) {
        self.pdf_effect = self.pdf_controller.last_page();
        self.apply_pdf_effect();
    }

    pub(crate) fn pdf_go_to_page(&mut self, page_number: u32) {
        let _ = self.go_to_page_active(page_number);
    }

    pub(crate) fn pdf_fit_width(&mut self) {
        let _ = self.set_active_view_mode(DocumentViewMode::PdfZoom(PdfZoomMode::FitWidth));
    }

    pub(crate) fn pdf_fit_page(&mut self) {
        let _ = self.set_active_view_mode(DocumentViewMode::PdfZoom(PdfZoomMode::FitPage));
    }

    pub(crate) fn pdf_reset_zoom(&mut self) {
        let _ = self.zoom_active_document_to(1.0);
    }

    pub(crate) fn pdf_set_viewport(&mut self, width_points: f32, height_points: f32) {
        self.pdf_effect = self
            .pdf_controller
            .set_viewport_points(width_points, height_points);
        self.apply_pdf_effect();
    }

    pub(crate) fn pdf_scroll(&mut self, delta_x: f32, delta_y: f32) {
        let _ = self.scroll_active_document(f64::from(delta_x), f64::from(delta_y));
    }

    pub(crate) fn pdf_handle_key(&mut self, key: KeyCode) {
        self.pdf_effect = self.pdf_controller.handle_key(key);
        self.apply_pdf_effect();
    }

    pub(crate) fn pdf_frame(&self) -> PdfViewerFrame {
        let state = self.pdf_controller.state();
        let placements = state.page_placements();
        let visible_ids = state.visible_page_ids();
        let pages = placements
            .into_iter()
            .filter(|placement| visible_ids.contains(&placement.page_index))
            .map(|placement| PdfFramePage {
                png: state
                    .visible_pages
                    .iter()
                    .find(|page| page.page_index == placement.page_index)
                    .map(|page| page.png.clone())
                    .unwrap_or_default(),
                placement,
            })
            .collect();
        PdfViewerFrame {
            document_id: state.document_id,
            status: state.status,
            error: state.error.clone(),
            current_page: state.current_page,
            current_page_number: state.current_page_number(),
            page_count: state.page_count,
            scale: state.scale,
            zoom_mode: state.zoom_mode,
            viewport_width_points: state.viewport_width_points,
            viewport_height_points: state.viewport_height_points,
            content_width_points: state.content_width_points,
            content_height_points: state.content_height_points,
            scroll_x_points: state.scroll_x_points,
            scroll_y_points: state.scroll_y_points,
            pages,
        }
    }

    pub(crate) fn pdf_zoom_in(&mut self) {
        let current = self.pdf_controller.state().scale;
        let _ = self.zoom_active_document_to(f64::from(current + 0.25));
    }

    pub(crate) fn pdf_zoom_out(&mut self) {
        let current = self.pdf_controller.state().scale;
        let _ = self.zoom_active_document_to(f64::from(current - 0.25));
    }
}
