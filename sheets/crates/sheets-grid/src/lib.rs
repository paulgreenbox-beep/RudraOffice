//! Minimal virtualized grid primitives and layout utilities for the sheet renderer.
//!
//! The module is renderer-neutral and centered on sparse storage with
//! index/offset calculations suitable for 100k+ row sheets.
//!
//! Public coordinate types are re-exported from [`sheets_core`] and are
//! treated as the canonical identity for rows, columns, addresses, and ranges.

use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod hidden_rows_tests;

#[cfg(test)]
mod hidden_columns_tests;

pub use sheets_core::{CellAddress, CellRange, ColumnIndex, RowIndex, SheetId};

/// Errors produced by grid state transitions and geometry validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridError {
    /// Requested index or offset is out of bounds for the current dimension.
    OutOfBounds {
        axis: &'static str,
        requested: u64,
        max: u64,
    },
    /// Grid dimension value is invalid.
    InvalidDimension { reason: &'static str },
    /// Override size does not satisfy axis constraints.
    InvalidAxisSize { reason: &'static str, value: u32 },
    /// Address references a different sheet.
    SheetMismatch { sheet: SheetId, expected: SheetId },
    /// Float input was unusable (NaN or infinite).
    InvalidFiniteInput { kind: &'static str },
    /// A requested `u32` extent cannot represent the exact axis extent.
    ExtentOverflow {
        axis: &'static str,
        extent: u128,
        max: u128,
    },
}

impl std::fmt::Display for GridError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfBounds {
                axis,
                requested,
                max,
            } => write!(f, "{axis} index {requested} out of bounds (max {max})"),
            Self::InvalidDimension { reason } => write!(f, "invalid grid dimension: {reason}"),
            Self::InvalidAxisSize { reason, value } => {
                write!(f, "invalid axis size {value}: {reason}")
            }
            Self::SheetMismatch { sheet, expected } => {
                write!(f, "sheet mismatch: got {sheet}, expected {expected}")
            }
            Self::InvalidFiniteInput { kind } => write!(f, "invalid finite input: {kind}"),
            Self::ExtentOverflow { axis, extent, max } => {
                write!(f, "{axis} extent {extent} exceeds maximum {max}")
            }
        }
    }
}

impl std::error::Error for GridError {}

/// Finite cell and viewport dimension count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridDimensions {
    pub rows: u32,
    pub columns: u32,
}

impl GridDimensions {
    /// Creates dimensions. Empty axes, including a `0x0` grid, are valid.
    pub fn new(rows: u32, columns: u32) -> Result<Self, GridError> {
        Ok(Self { rows, columns })
    }

    pub fn contains_row(&self, row: RowIndex) -> bool {
        row.0 < self.rows
    }

    pub fn contains_col(&self, col: ColumnIndex) -> bool {
        col.0 < self.columns
    }

    pub fn validate_address(&self, address: CellAddress) -> Result<(), GridError> {
        if !self.contains_row(address.row) {
            return Err(GridError::OutOfBounds {
                axis: "row",
                requested: u64::from(address.row.0),
                max: u64::from(self.rows),
            });
        }
        if !self.contains_col(address.column) {
            return Err(GridError::OutOfBounds {
                axis: "column",
                requested: u64::from(address.column.0),
                max: u64::from(self.columns),
            });
        }
        Ok(())
    }
}

/// Inclusive index interval used for visible row/column regions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexRange {
    pub start: u32,
    pub end: u32,
}

impl IndexRange {
    pub fn new(start: u32, end: u32) -> Option<Self> {
        if start <= end {
            Some(Self { start, end })
        } else {
            None
        }
    }

    pub fn len(&self) -> u32 {
        if self.is_empty() {
            0
        } else {
            self.end - self.start + 1
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start > self.end
    }
}

/// Sparse matrix keyed by canonical (row, column) index pair.
#[derive(Debug)]
pub struct SparseGrid<T> {
    sheet: SheetId,
    cells: BTreeMap<(RowIndex, ColumnIndex), T>,
}

impl<T> Default for SparseGrid<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> SparseGrid<T> {
    /// Creates a sparse grid for the default sheet. Use [`Self::for_sheet`]
    /// when the owner is not `SheetId(0)`.
    pub fn new() -> Self {
        Self::for_sheet(SheetId(0))
    }

    /// Creates a sparse grid with an explicit sheet owner.
    pub fn for_sheet(sheet: SheetId) -> Self {
        Self {
            sheet,
            cells: BTreeMap::new(),
        }
    }

    pub fn sheet(&self) -> SheetId {
        self.sheet
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn clear(&mut self) {
        self.cells.clear()
    }

    pub fn insert(
        &mut self,
        address: CellAddress,
        value: T,
        bounds: &GridDimensions,
    ) -> Result<Option<T>, GridError> {
        self.validate_address(address, bounds)?;
        Ok(self.cells.insert((address.row, address.column), value))
    }

    pub fn get(&self, address: CellAddress, bounds: &GridDimensions) -> Option<&T> {
        self.validate_address(address, bounds).ok()?;
        self.cells.get(&(address.row, address.column))
    }

    pub fn get_mut(&mut self, address: CellAddress, bounds: &GridDimensions) -> Option<&mut T> {
        self.validate_address(address, bounds).ok()?;
        self.cells.get_mut(&(address.row, address.column))
    }

    pub fn remove(&mut self, address: CellAddress, bounds: &GridDimensions) -> Option<T> {
        self.validate_address(address, bounds).ok()?;
        self.cells.remove(&(address.row, address.column))
    }

    pub fn iter(&self) -> impl Iterator<Item = (CellAddress, &T)> + '_ {
        self.cells
            .iter()
            .map(|((row, col), value)| (CellAddress::new(self.sheet, *row, *col), value))
    }

    pub fn retain_in_bounds(&mut self, bounds: &GridDimensions) {
        self.cells
            .retain(|(row, col), _| row.0 < bounds.rows && col.0 < bounds.columns);
    }

    fn validate_address(
        &self,
        address: CellAddress,
        bounds: &GridDimensions,
    ) -> Result<(), GridError> {
        if address.sheet != self.sheet {
            return Err(GridError::SheetMismatch {
                sheet: address.sheet,
                expected: self.sheet,
            });
        }
        bounds.validate_address(address)
    }
}

/// Point in logical view coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x: sanitize_coordinate(x),
            y: sanitize_coordinate(y),
        }
    }

    fn is_valid(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.x >= 0.0 && self.y >= 0.0
    }
}

/// Rectangle in logical view coordinates. Origins may be negative after scrolling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        let valid_origin = x.is_finite() && y.is_finite();
        Self {
            x: if x.is_finite() { x } else { -1.0 },
            y: if y.is_finite() { y } else { -1.0 },
            w: if valid_origin {
                sanitize_extent(w)
            } else {
                -1.0
            },
            h: if valid_origin {
                sanitize_extent(h)
            } else {
                -1.0
            },
        }
    }

    pub fn right(&self) -> f64 {
        finite_sum(self.x, self.w)
    }

    pub fn bottom(&self) -> f64 {
        finite_sum(self.y, self.h)
    }

    pub fn contains(&self, point: Point) -> bool {
        self.is_valid()
            && point.is_valid()
            && point.x >= self.x
            && point.y >= self.y
            && point.x < self.right()
            && point.y < self.bottom()
    }

    /// Returns the nonempty overlap with another rectangle.
    pub fn intersection(&self, other: Self) -> Option<Self> {
        if !self.is_valid() || !other.is_valid() {
            return None;
        }
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| Self::new(x, y, right - x, bottom - y))
    }

    fn is_valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.w.is_finite()
            && self.h.is_finite()
            && self.w >= 0.0
            && self.h >= 0.0
    }
}

/// Viewport rectangle in logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub origin: Point,
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            origin: Point::new(x, y),
            width: sanitize_dimension(width),
            height: sanitize_dimension(height),
        }
    }

    fn is_valid(self) -> bool {
        self.origin.is_valid() && self.width > 0 && self.height > 0
    }
}

/// Scroll offset in logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ScrollOffset {
    pub x: u32,
    pub y: u32,
}

impl ScrollOffset {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x: sanitize_scroll_dimension(x),
            y: sanitize_scroll_dimension(y),
        }
    }

    fn is_valid(self) -> bool {
        self.x != u32::MAX && self.y != u32::MAX
    }
}

#[inline]
fn sanitize_coordinate(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        -1.0
    }
}

#[inline]
fn sanitize_extent(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        -1.0
    }
}

#[inline]
fn sanitize_dimension(value: f64) -> u32 {
    if value.is_finite() && value >= 0.0 {
        value.min(u32::MAX as f64) as u32
    } else {
        0
    }
}

#[inline]
fn sanitize_scroll_dimension(value: f64) -> u32 {
    if value.is_finite() && value >= 0.0 {
        value.min(f64::from(u32::MAX - 1)) as u32
    } else {
        u32::MAX
    }
}

#[inline]
fn finite_sum(left: f64, right: f64) -> f64 {
    let sum = left + right;
    if sum.is_finite() { sum } else { f64::MAX }
}

/// Frozen pane strategy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrozenPanes {
    pub rows: u32,
    pub cols: u32,
}

impl FrozenPanes {
    pub fn with(rows: u32, cols: u32) -> Self {
        Self { rows, cols }
    }

    pub fn clamp_to_dimensions(&self, dimensions: &GridDimensions) -> Self {
        Self {
            rows: self.rows.min(dimensions.rows),
            cols: self.cols.min(dimensions.columns),
        }
    }
}

/// Optional overscan around viewport when computing visible intervals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Overscan {
    pub rows: u32,
    pub cols: u32,
}

impl Default for Overscan {
    fn default() -> Self {
        Self { rows: 2, cols: 2 }
    }
}

/// Variable-size axis backed by sparse overrides and per-block deltas.
#[derive(Debug)]
pub struct VariableAxis {
    default_size: u32,
    min_size: u32,
    max_size: u32,
    overrides: BTreeMap<u32, u32>,
    hidden: BTreeSet<u32>,
    block_size: u32,
    block_delta: BTreeMap<u32, i64>,
}

impl VariableAxis {
    pub fn new(default_size: u32, min_size: u32, max_size: u32) -> Result<Self, GridError> {
        if default_size == 0 {
            return Err(GridError::InvalidAxisSize {
                reason: "default size must be greater than zero",
                value: default_size,
            });
        }
        if min_size == 0 {
            return Err(GridError::InvalidAxisSize {
                reason: "minimum size must be greater than zero",
                value: min_size,
            });
        }
        if min_size > max_size {
            return Err(GridError::InvalidAxisSize {
                reason: "minimum size cannot exceed maximum size",
                value: min_size,
            });
        }
        if default_size < min_size || default_size > max_size {
            return Err(GridError::InvalidAxisSize {
                reason: "default size must satisfy bounds",
                value: default_size,
            });
        }
        Ok(Self {
            default_size,
            min_size,
            max_size,
            overrides: BTreeMap::new(),
            hidden: BTreeSet::new(),
            block_size: 256,
            block_delta: BTreeMap::new(),
        })
    }

    pub fn default_size(&self) -> u32 {
        self.default_size
    }

    pub fn min_size(&self) -> u32 {
        self.min_size
    }

    pub fn max_size(&self) -> u32 {
        self.max_size
    }

    pub fn is_override(&self, index: u32) -> bool {
        self.overrides.contains_key(&index)
    }

    fn validate_index_bounds(index: u32, axis_len: u32) -> Result<(), GridError> {
        if index >= axis_len {
            return Err(GridError::OutOfBounds {
                axis: "axis index",
                requested: u64::from(index),
                max: u64::from(axis_len),
            });
        }
        Ok(())
    }

    pub fn get(&self, index: u32) -> Option<u32> {
        self.overrides.get(&index).copied()
    }

    pub fn set_override(
        &mut self,
        index: u32,
        size: u32,
        axis_len: u32,
    ) -> Result<Option<u32>, GridError> {
        Self::validate_index_bounds(index, axis_len)?;

        if size < self.min_size {
            return Err(GridError::InvalidAxisSize {
                reason: "override below minimum size",
                value: size,
            });
        }
        if size > self.max_size {
            return Err(GridError::InvalidAxisSize {
                reason: "override above maximum size",
                value: size,
            });
        }

        let old = self.overrides.get(&index).copied();
        let old_size = old.unwrap_or(self.default_size);
        if size == self.default_size {
            self.overrides.remove(&index);
        } else {
            self.overrides.insert(index, size);
        }
        if !self.is_hidden(index) {
            self.adjust_block_delta(index, size as i64 - old_size as i64);
        }
        Ok(old)
    }

    pub fn remove_override(&mut self, index: u32, axis_len: u32) -> Result<Option<u32>, GridError> {
        Self::validate_index_bounds(index, axis_len)?;
        let old = self.overrides.remove(&index);
        if let Some(old_size) = old
            && !self.is_hidden(index)
        {
            self.adjust_block_delta(index, self.default_size as i64 - old_size as i64);
        }
        Ok(old)
    }

    pub fn size(&self, index: u32) -> u32 {
        if self.is_hidden(index) {
            return 0;
        }
        self.overrides
            .get(&index)
            .copied()
            .unwrap_or(self.default_size)
    }

    /// Clears configured sizes while retaining hidden indices.
    pub fn clear_overrides(&mut self) -> bool {
        if self.overrides.is_empty() {
            return false;
        }
        self.overrides.clear();
        self.rebuild_block_deltas();
        true
    }

    /// Replaces a sparse visibility mask atomically. Hidden indices retain
    /// their positive configured dimensions and have zero effective extent.
    pub fn set_hidden(&mut self, indices: &[u32], axis_len: u32) -> Result<(), GridError> {
        for &index in indices {
            Self::validate_index_bounds(index, axis_len)?;
        }
        let hidden = indices.iter().copied().collect();
        if self.hidden != hidden {
            self.hidden = hidden;
            self.rebuild_block_deltas();
        }
        Ok(())
    }

    pub fn is_hidden(&self, index: u32) -> bool {
        self.hidden.contains(&index)
    }

    /// Inclusive next visible index; work depends on the sparse hidden run.
    pub fn next_visible(&self, index: u32, axis_len: u32) -> Option<u32> {
        if index >= axis_len {
            return None;
        }
        let mut candidate = index;
        for &hidden in self.hidden.range(index..axis_len) {
            if hidden != candidate {
                break;
            }
            candidate = candidate.checked_add(1)?;
        }
        (candidate < axis_len).then_some(candidate)
    }

    /// Inclusive previous visible index, clamped to the axis bounds.
    pub fn previous_visible(&self, index: u32, axis_len: u32) -> Option<u32> {
        let mut candidate = index.min(axis_len.checked_sub(1)?);
        for &hidden in self.hidden.range(..=candidate).rev() {
            if hidden != candidate {
                break;
            }
            candidate = candidate.checked_sub(1)?;
        }
        Some(candidate)
    }

    pub fn visible_indices(
        &self,
        range: IndexRange,
        axis_len: u32,
    ) -> impl Iterator<Item = u32> + '_ {
        std::iter::successors(self.next_visible(range.start, axis_len), move |index| {
            self.next_visible(index.checked_add(1)?, axis_len)
        })
        .take_while(move |index| *index <= range.end)
    }

    fn partial_delta(&self, start: u32, end: u32) -> i128 {
        let dimensions: i128 = self
            .overrides
            .range(start..end)
            .filter(|(index, _)| !self.is_hidden(**index))
            .map(|(_, size)| i128::from(*size) - i128::from(self.default_size))
            .sum();
        dimensions - self.hidden.range(start..end).count() as i128 * i128::from(self.default_size)
    }

    fn rebuild_block_deltas(&mut self) {
        self.block_delta.clear();
        let dimensions: Vec<_> = self
            .overrides
            .iter()
            .filter(|(index, _)| !self.is_hidden(**index))
            .map(|(index, size)| (*index, i64::from(*size) - i64::from(self.default_size)))
            .collect();
        for (index, delta) in dimensions {
            self.adjust_block_delta(index, delta);
        }
        let hidden: Vec<_> = self.hidden.iter().copied().collect();
        for index in hidden {
            self.adjust_block_delta(index, -i64::from(self.default_size));
        }
    }

    fn adjust_block_delta(&mut self, index: u32, delta: i64) {
        if delta == 0 {
            return;
        }
        let block = index / self.block_size;
        let new_delta = self.block_delta.get(&block).copied().unwrap_or(0) + delta;
        if new_delta == 0 {
            self.block_delta.remove(&block);
        } else {
            self.block_delta.insert(block, new_delta);
        }
    }

    fn block_start(&self, block: u32) -> u32 {
        block * self.block_size
    }

    fn block_count(&self, axis_len: u32) -> u32 {
        axis_len.div_ceil(self.block_size)
    }

    fn prefix_delta(&self, block: u32) -> i128 {
        self.block_delta
            .range(..block)
            .map(|(_, delta)| i128::from(*delta))
            .sum()
    }

    /// Returns the exact extent. This is the lossless API for large sheets.
    pub fn total_extent_exact(&self, axis_len: u32) -> Result<u128, GridError> {
        self.offset_for_index_exact(axis_len, axis_len)
    }

    /// Returns a `u32` extent or an explicit error instead of saturating.
    pub fn total_extent(&self, axis_len: u32) -> Result<u32, GridError> {
        let extent = self.total_extent_exact(axis_len)?;
        u32::try_from(extent).map_err(|_| GridError::ExtentOverflow {
            axis: "axis",
            extent,
            max: u128::from(u32::MAX),
        })
    }

    fn offset_for_index_exact(&self, index: u32, axis_len: u32) -> Result<u128, GridError> {
        if index > axis_len {
            return Err(GridError::OutOfBounds {
                axis: "axis index",
                requested: u64::from(index),
                max: u64::from(axis_len),
            });
        }
        if index == 0 {
            return Ok(0);
        }

        let block = index / self.block_size;
        let block_start = self.block_start(block);
        let base = u128::from(index) * u128::from(self.default_size);
        let offset =
            i128::try_from(base).expect("axis offset fits in i128") + self.prefix_delta(block);
        let offset = offset + self.partial_delta(block_start, index);
        u128::try_from(offset).map_err(|_| GridError::InvalidDimension {
            reason: "axis offset became negative",
        })
    }

    pub fn offset_for_index(&self, index: u32, axis_len: u32) -> Result<u32, GridError> {
        let offset = self.offset_for_index_exact(index, axis_len)?;
        u32::try_from(offset).map_err(|_| GridError::ExtentOverflow {
            axis: "axis offset",
            extent: offset,
            max: u128::from(u32::MAX),
        })
    }

    fn index_for_offset_exact(&self, offset: u128, axis_len: u32) -> Result<u32, GridError> {
        let total = self.total_extent_exact(axis_len)?;
        if axis_len == 0 || offset >= total {
            return Err(GridError::OutOfBounds {
                axis: "axis offset",
                requested: u64::try_from(offset).unwrap_or(u64::MAX),
                max: u64::try_from(total).unwrap_or(u64::MAX),
            });
        }

        let block_count = self.block_count(axis_len);
        let mut low = 0;
        let mut high = block_count;
        while low < high {
            let block = low + (high - low) / 2;
            let end = ((u64::from(block) + 1) * u64::from(self.block_size)).min(u64::from(axis_len))
                as u32;
            if self.offset_for_index_exact(end, axis_len)? <= offset {
                low = block + 1;
            } else {
                high = block;
            }
        }
        let block_start_idx = self.block_start(low);
        let mut cursor_idx = block_start_idx;
        let mut cursor_offset = self.offset_for_index_exact(block_start_idx, axis_len)?;
        while cursor_idx < axis_len {
            let next_offset = cursor_offset + u128::from(self.size(cursor_idx));
            if offset < next_offset {
                return Ok(cursor_idx);
            }
            cursor_idx += 1;
            cursor_offset = next_offset;
        }
        Err(GridError::OutOfBounds {
            axis: "axis offset",
            requested: u64::try_from(offset).unwrap_or(u64::MAX),
            max: u64::try_from(total).unwrap_or(u64::MAX),
        })
    }

    pub fn offset_to_index(&self, offset: u64, axis_len: u32) -> Result<u32, GridError> {
        self.index_for_offset_exact(u128::from(offset), axis_len)
    }

    pub fn visible_interval(
        &self,
        start_index: u32,
        axis_len: u32,
        scroll_offset: u32,
        viewport_size: u32,
        overscan: u32,
    ) -> Result<Option<IndexRange>, GridError> {
        if axis_len == 0 || viewport_size == 0 || start_index >= axis_len {
            return Ok(None);
        }

        let start_offset = self.offset_for_index_exact(start_index, axis_len)?;
        let total_extent = self.total_extent_exact(axis_len)?;
        let body_extent = total_extent.saturating_sub(start_offset);
        let scroll = u128::from(scroll_offset);
        let window_start = start_offset + scroll;
        if scroll >= body_extent || window_start >= total_extent {
            return Ok(None);
        }

        let request_end = window_start
            .saturating_add(u128::from(viewport_size))
            .saturating_add(u128::from(overscan));
        let window_end = request_end.min(total_extent - 1);
        let first = self.index_for_offset_exact(window_start, axis_len)?;
        let last = self.index_for_offset_exact(window_end, axis_len)?;

        Ok(IndexRange::new(first, last))
    }

    pub fn clear_overrides_beyond(&mut self, axis_len: u32) {
        self.overrides.retain(|idx, _| *idx < axis_len);
        self.hidden.retain(|index| *index < axis_len);
        self.rebuild_block_deltas();
    }
}

/// Renderer-agnostic geometry queries.
pub trait GridGeometry {
    fn layout(&self, viewport: Viewport, scroll: ScrollOffset) -> LayoutReport;
    /// Returns a cell rectangle in document coordinates, independent of view scroll.
    fn cell_rect(&self, address: CellAddress) -> Option<Rect>;
    fn hit_test(
        &self,
        point: Point,
        viewport: Viewport,
        scroll: ScrollOffset,
    ) -> Option<CellAddress>;
}

/// Layout output for row/column visibility and exact content extents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutReport {
    pub viewport: Viewport,
    pub frozen_rows: Option<IndexRange>,
    pub frozen_cols: Option<IndexRange>,
    pub body_rows: Option<IndexRange>,
    pub body_cols: Option<IndexRange>,
    pub content_width: u128,
    pub content_height: u128,
}

pub struct VirtualGrid<T> {
    sheet: SheetId,
    dimensions: GridDimensions,
    cells: SparseGrid<T>,
    row_sizes: VariableAxis,
    column_sizes: VariableAxis,
    frozen: FrozenPanes,
    overscan: Overscan,
}

impl<T> VirtualGrid<T> {
    pub fn new(
        sheet: SheetId,
        dimensions: GridDimensions,
        row_default_size: u32,
        column_default_size: u32,
    ) -> Result<Self, GridError> {
        let row_sizes = VariableAxis::new(row_default_size, 1, 4096)?;
        let column_sizes = VariableAxis::new(column_default_size, 1, 4096)?;
        Ok(Self {
            sheet,
            dimensions,
            cells: SparseGrid::for_sheet(sheet),
            row_sizes,
            column_sizes,
            frozen: FrozenPanes::default(),
            overscan: Overscan::default(),
        })
    }

    pub fn dimensions(&self) -> GridDimensions {
        self.dimensions
    }

    pub fn set_overscan(&mut self, overscan: Overscan) {
        self.overscan = overscan;
    }

    pub fn set_frozen_panes(&mut self, frozen: FrozenPanes) {
        self.frozen = frozen;
    }

    pub fn set_row_size(&mut self, row: RowIndex, size: u32) -> Result<Option<u32>, GridError> {
        self.row_sizes
            .set_override(row.0, size, self.dimensions.rows)
    }

    pub fn set_hidden_rows(&mut self, rows: &[RowIndex]) -> Result<(), GridError> {
        let indices: Vec<_> = rows.iter().map(|row| row.0).collect();
        self.row_sizes.set_hidden(&indices, self.dimensions.rows)
    }

    pub fn set_hidden_columns(&mut self, columns: &[ColumnIndex]) -> Result<(), GridError> {
        let indices: Vec<_> = columns.iter().map(|column| column.0).collect();
        self.column_sizes
            .set_hidden(&indices, self.dimensions.columns)
    }

    /// Removes old row and column sizes before installing dimensions shifted
    /// by a structural operation. Visibility masks are preserved.
    pub fn clear_axis_overrides(&mut self) -> bool {
        self.row_sizes.clear_overrides() | self.column_sizes.clear_overrides()
    }

    pub fn is_row_hidden(&self, row: RowIndex) -> bool {
        self.row_sizes.is_hidden(row.0)
    }

    pub fn hidden_rows(&self) -> impl Iterator<Item = RowIndex> + '_ {
        self.row_sizes.hidden.iter().copied().map(RowIndex)
    }

    pub fn next_visible_row(&self, row: RowIndex) -> Option<RowIndex> {
        self.row_sizes
            .next_visible(row.0, self.dimensions.rows)
            .map(RowIndex)
    }

    pub fn previous_visible_row(&self, row: RowIndex) -> Option<RowIndex> {
        self.row_sizes
            .previous_visible(row.0, self.dimensions.rows)
            .map(RowIndex)
    }

    pub fn visible_rows(&self, range: IndexRange) -> impl Iterator<Item = RowIndex> + '_ {
        self.row_sizes
            .visible_indices(range, self.dimensions.rows)
            .map(RowIndex)
    }

    pub fn is_column_hidden(&self, column: ColumnIndex) -> bool {
        self.column_sizes.is_hidden(column.0)
    }

    pub fn hidden_columns(&self) -> impl Iterator<Item = ColumnIndex> + '_ {
        self.column_sizes.hidden.iter().copied().map(ColumnIndex)
    }

    pub fn next_visible_column(&self, column: ColumnIndex) -> Option<ColumnIndex> {
        self.column_sizes
            .next_visible(column.0, self.dimensions.columns)
            .map(ColumnIndex)
    }

    pub fn previous_visible_column(&self, column: ColumnIndex) -> Option<ColumnIndex> {
        self.column_sizes
            .previous_visible(column.0, self.dimensions.columns)
            .map(ColumnIndex)
    }

    pub fn visible_columns(&self, range: IndexRange) -> impl Iterator<Item = ColumnIndex> + '_ {
        self.column_sizes
            .visible_indices(range, self.dimensions.columns)
            .map(ColumnIndex)
    }

    /// Clamp logical scroll to the effective (visible) content extent.
    pub fn clamp_scroll(&self, viewport: Viewport, scroll: ScrollOffset) -> ScrollOffset {
        let max_x = self
            .column_sizes
            .total_extent_exact(self.dimensions.columns)
            .unwrap_or(0)
            .saturating_sub(u128::from(viewport.width));
        let max_y = self
            .row_sizes
            .total_extent_exact(self.dimensions.rows)
            .unwrap_or(0)
            .saturating_sub(u128::from(viewport.height));
        ScrollOffset {
            x: scroll.x.min(u32::try_from(max_x).unwrap_or(u32::MAX)),
            y: scroll.y.min(u32::try_from(max_y).unwrap_or(u32::MAX)),
        }
    }

    pub fn set_column_size(
        &mut self,
        col: ColumnIndex,
        size: u32,
    ) -> Result<Option<u32>, GridError> {
        self.column_sizes
            .set_override(col.0, size, self.dimensions.columns)
    }

    pub fn remove_row_size_override(&mut self, row: RowIndex) -> Result<Option<u32>, GridError> {
        self.row_sizes.remove_override(row.0, self.dimensions.rows)
    }

    pub fn remove_column_size_override(
        &mut self,
        col: ColumnIndex,
    ) -> Result<Option<u32>, GridError> {
        self.column_sizes
            .remove_override(col.0, self.dimensions.columns)
    }

    pub fn insert_cell(&mut self, address: CellAddress, value: T) -> Result<Option<T>, GridError> {
        self.cells.insert(address, value, &self.dimensions)
    }

    pub fn remove_cell(&mut self, address: CellAddress) -> Option<T> {
        self.cells.remove(address, &self.dimensions)
    }

    pub fn cell(&self, address: CellAddress) -> Option<&T> {
        self.cells.get(address, &self.dimensions)
    }

    /// Returns the full inclusive range rectangle in document coordinates.
    /// Uses exact axis offsets, including overrides outside the visible viewport.
    pub fn range_rect(&self, range: &CellRange) -> Option<Rect> {
        range.validate().ok()?;
        if range.sheet != self.sheet {
            return None;
        }
        let (x, y, _, _) = self.cell_bounds(range.start)?;
        let (end_x, end_y, end_w, end_h) = self.cell_bounds(range.end)?;
        let width = end_x.checked_add(u128::from(end_w))?.checked_sub(x)?;
        let height = end_y.checked_add(u128::from(end_h))?.checked_sub(y)?;
        Some(Rect::new(x as f64, y as f64, width as f64, height as f64))
    }

    pub fn resize(&mut self, dimensions: GridDimensions) {
        self.dimensions = dimensions;
        self.cells.retain_in_bounds(&self.dimensions);
        self.row_sizes.clear_overrides_beyond(self.dimensions.rows);
        self.column_sizes
            .clear_overrides_beyond(self.dimensions.columns);
    }

    fn cell_bounds(&self, address: CellAddress) -> Option<(u128, u128, u32, u32)> {
        self.dimensions.validate_address(address).ok()?;
        if address.sheet != self.sheet {
            return None;
        }
        let x = self
            .column_sizes
            .offset_for_index_exact(address.column.0, self.dimensions.columns)
            .ok()?;
        let y = self
            .row_sizes
            .offset_for_index_exact(address.row.0, self.dimensions.rows)
            .ok()?;
        let w = self.column_sizes.size(address.column.0);
        let h = self.row_sizes.size(address.row.0);
        Some((x, y, w, h))
    }

    pub fn row_axis_default(&self) -> u32 {
        self.row_sizes.default_size()
    }

    pub fn col_axis_default(&self) -> u32 {
        self.column_sizes.default_size()
    }

    fn index_at_offset(axis: &VariableAxis, offset: u128, axis_len: u32) -> Option<u32> {
        let offset = u64::try_from(offset).ok()?;
        axis.offset_to_index(offset, axis_len).ok()
    }
}

impl<T> GridGeometry for VirtualGrid<T> {
    fn layout(&self, viewport: Viewport, scroll: ScrollOffset) -> LayoutReport {
        let frozen = self.frozen.clamp_to_dimensions(&self.dimensions);
        let frozen_rows = self
            .row_sizes
            .visible_interval(0, frozen.rows, 0, viewport.height, self.overscan.rows)
            .unwrap_or_default();
        let frozen_cols = self
            .column_sizes
            .visible_interval(0, frozen.cols, 0, viewport.width, self.overscan.cols)
            .unwrap_or_default();

        let frozen_height = self
            .row_sizes
            .total_extent_exact(frozen.rows)
            .unwrap_or_default();
        let frozen_width = self
            .column_sizes
            .total_extent_exact(frozen.cols)
            .unwrap_or_default();
        let body_height = u128::from(viewport.height).saturating_sub(frozen_height);
        let body_width = u128::from(viewport.width).saturating_sub(frozen_width);
        let body_height = u32::try_from(body_height).unwrap_or(u32::MAX);
        let body_width = u32::try_from(body_width).unwrap_or(u32::MAX);

        let body_rows = self
            .row_sizes
            .visible_interval(
                frozen.rows,
                self.dimensions.rows,
                scroll.y,
                body_height,
                self.overscan.rows,
            )
            .unwrap_or_default();
        let body_cols = self
            .column_sizes
            .visible_interval(
                frozen.cols,
                self.dimensions.columns,
                scroll.x,
                body_width,
                self.overscan.cols,
            )
            .unwrap_or_default();

        let content_width = self
            .column_sizes
            .total_extent_exact(self.dimensions.columns)
            .unwrap_or_default();
        let content_height = self
            .row_sizes
            .total_extent_exact(self.dimensions.rows)
            .unwrap_or_default();

        LayoutReport {
            viewport,
            frozen_rows,
            frozen_cols,
            body_rows,
            body_cols,
            content_width,
            content_height,
        }
    }

    fn cell_rect(&self, address: CellAddress) -> Option<Rect> {
        let (x, y, w, h) = self.cell_bounds(address)?;
        Some(Rect::new(x as f64, y as f64, w as f64, h as f64))
    }

    fn hit_test(
        &self,
        point: Point,
        viewport: Viewport,
        scroll: ScrollOffset,
    ) -> Option<CellAddress> {
        if !point.is_valid() || !viewport.is_valid() || !scroll.is_valid() {
            return None;
        }
        let local_x = point.x - viewport.origin.x;
        let local_y = point.y - viewport.origin.y;
        if !local_x.is_finite()
            || !local_y.is_finite()
            || local_x < 0.0
            || local_y < 0.0
            || local_x >= f64::from(viewport.width)
            || local_y >= f64::from(viewport.height)
        {
            return None;
        }

        let frozen = self.frozen.clamp_to_dimensions(&self.dimensions);
        let frozen_width = self.column_sizes.total_extent_exact(frozen.cols).ok()?;
        let frozen_height = self.row_sizes.total_extent_exact(frozen.rows).ok()?;
        let x = u128::from(local_x.floor() as u32);
        let y = u128::from(local_y.floor() as u32);
        let in_frozen_cols = x < frozen_width && frozen.cols > 0;
        let in_frozen_rows = y < frozen_height && frozen.rows > 0;

        let column_offset = if in_frozen_cols {
            x
        } else if frozen.cols < self.dimensions.columns {
            x.checked_add(u128::from(scroll.x))?
        } else {
            return None;
        };
        let row_offset = if in_frozen_rows {
            y
        } else if frozen.rows < self.dimensions.rows {
            y.checked_add(u128::from(scroll.y))?
        } else {
            return None;
        };

        let column =
            Self::index_at_offset(&self.column_sizes, column_offset, self.dimensions.columns)?;
        let row = Self::index_at_offset(&self.row_sizes, row_offset, self.dimensions.rows)?;
        Some(CellAddress::new(
            self.sheet,
            RowIndex(row),
            ColumnIndex(column),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_grid(rows: u32, cols: u32) -> VirtualGrid<String> {
        VirtualGrid::new(
            SheetId(0),
            GridDimensions {
                rows,
                columns: cols,
            },
            20,
            12,
        )
        .unwrap()
    }

    #[test]
    fn zero_dimensions_are_valid_and_have_no_cells_or_hits() {
        let grid = make_grid(0, 0);
        assert_eq!(GridDimensions::new(0, 0).unwrap(), grid.dimensions());
        assert!(
            grid.hit_test(
                Point::new(1.0, 1.0),
                Viewport::new(0.0, 0.0, 10.0, 10.0),
                ScrollOffset::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn geometry_zero_frozen_layout_does_not_add_origin_axes() {
        let grid = make_grid(20, 20);
        let layout = grid.layout(
            Viewport::new(0.0, 0.0, 24.0, 40.0),
            ScrollOffset::new(60.0, 100.0),
        );
        assert_eq!(layout.frozen_rows, None);
        assert_eq!(layout.frozen_cols, None);
        assert_eq!(layout.body_rows.unwrap().start, 5);
        assert_eq!(layout.body_cols.unwrap().start, 5);

        let empty =
            make_grid(0, 0).layout(Viewport::new(0.0, 0.0, 24.0, 40.0), ScrollOffset::default());
        assert_eq!(empty.frozen_rows, None);
        assert_eq!(empty.frozen_cols, None);
        assert_eq!(empty.body_rows, None);
        assert_eq!(empty.body_cols, None);
    }

    #[test]
    fn geometry_range_rect_sums_all_variable_axis_sizes() {
        let mut grid = make_grid(10, 10);
        grid.set_row_size(RowIndex(1), 35).unwrap();
        grid.set_row_size(RowIndex(2), 45).unwrap();
        grid.set_column_size(ColumnIndex(1), 40).unwrap();
        grid.set_column_size(ColumnIndex(2), 55).unwrap();
        let mut range = CellRange::new(
            CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(1)),
            CellAddress::new(SheetId(0), RowIndex(3), ColumnIndex(3)),
        )
        .unwrap();
        assert_eq!(
            grid.range_rect(&range),
            Some(Rect::new(12.0, 20.0, 107.0, 100.0))
        );
        range.sheet = SheetId(1);
        assert_eq!(grid.range_rect(&range), None);
        range.sheet = SheetId(0);
        range.end.row = RowIndex(10);
        assert_eq!(grid.range_rect(&range), None);
    }

    #[test]
    fn geometry_signed_rect_preserves_extent_and_intersection() {
        let rect = Rect::new(-200.0, -40.0, 320.0, 100.0);
        assert_eq!(rect.x, -200.0);
        assert_eq!(rect.y, -40.0);
        assert_eq!(rect.right(), 120.0);
        assert_eq!(rect.bottom(), 60.0);
        assert!(rect.contains(Point::new(49.0, 57.0)));
        assert_eq!(
            rect.intersection(Rect::new(48.0, 56.0, 100.0, 80.0)),
            Some(Rect::new(48.0, 56.0, 72.0, 4.0)),
        );
        assert_eq!(rect.intersection(Rect::new(120.0, 0.0, 10.0, 10.0)), None);
        let invalid = Rect::new(f64::INFINITY, 0.0, 320.0, 100.0);
        assert!(invalid.x.is_finite() && invalid.w.is_finite());
        assert_eq!(invalid.intersection(rect), None);
    }

    #[test]
    fn sparse_grid_stores_only_materialized_cells_and_preserves_sheet() {
        let mut grid: VirtualGrid<String> = VirtualGrid::new(
            SheetId(7),
            GridDimensions {
                rows: 3,
                columns: 3,
            },
            20,
            12,
        )
        .unwrap();
        grid.insert_cell(
            CellAddress::new(SheetId(7), RowIndex(1), ColumnIndex(2)),
            "a".into(),
        )
        .unwrap();
        grid.insert_cell(
            CellAddress::new(SheetId(7), RowIndex(0), ColumnIndex(0)),
            "b".into(),
        )
        .unwrap();

        assert_eq!(grid.cells.len(), 2);
        assert_eq!(
            grid.cell(CellAddress::new(SheetId(7), RowIndex(1), ColumnIndex(2)))
                .cloned(),
            Some("a".into())
        );
        assert!(
            grid.cell(CellAddress::new(SheetId(7), RowIndex(2), ColumnIndex(1)))
                .is_none()
        );
        assert!(
            grid.cells
                .iter()
                .all(|(address, _)| address.sheet == SheetId(7))
        );
    }

    #[test]
    fn sparse_grid_rejects_out_of_bounds_and_resizes() {
        let mut grid = make_grid(2, 2);
        let err = grid
            .insert_cell(
                CellAddress::new(SheetId(0), RowIndex(2), ColumnIndex(0)),
                "x".into(),
            )
            .expect_err("expected out of bounds");
        assert!(matches!(err, GridError::OutOfBounds { axis: "row", .. }));

        grid.insert_cell(
            CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(1)),
            "ok".into(),
        )
        .unwrap();
        assert_eq!(grid.cells.len(), 1);

        grid.resize(GridDimensions {
            rows: 1,
            columns: 1,
        });
        assert_eq!(grid.cells.len(), 0);
    }

    #[test]
    fn grid_dimensions_and_resizing_are_bounds_checked() {
        let mut grid = make_grid(4, 5);
        assert!(grid.set_row_size(RowIndex(99), 24).is_err());
        assert!(grid.set_column_size(ColumnIndex(99), 24).is_err());

        grid.resize(GridDimensions {
            rows: 2,
            columns: 3,
        });
        assert_eq!(
            grid.dimensions(),
            GridDimensions {
                rows: 2,
                columns: 3
            }
        );
        assert!(grid.set_row_size(RowIndex(3), 24).is_err());
    }

    #[test]
    fn variable_axis_offsets_and_inverse_boundaries() {
        let mut axis = VariableAxis::new(10, 1, 100).unwrap();
        axis.set_override(1, 15, 3).unwrap();

        assert_eq!(axis.offset_for_index(0, 3).unwrap(), 0);
        assert_eq!(axis.offset_for_index(1, 3).unwrap(), 10);
        assert_eq!(axis.offset_for_index(2, 3).unwrap(), 25);
        assert_eq!(axis.total_extent(3).unwrap(), 35);

        assert_eq!(axis.offset_to_index(0, 3).unwrap(), 0);
        assert_eq!(axis.offset_to_index(9, 3).unwrap(), 0);
        assert_eq!(axis.offset_to_index(10, 3).unwrap(), 1);
        assert_eq!(axis.offset_to_index(24, 3).unwrap(), 1);
        assert_eq!(axis.offset_to_index(25, 3).unwrap(), 2);
        assert!(axis.offset_to_index(35, 3).is_err());
    }

    #[test]
    fn variable_axis_negative_delta_and_reset_default_are_exact() {
        let mut axis = VariableAxis::new(10, 1, 100).unwrap();
        axis.set_override(0, 5, 4).unwrap();
        axis.set_override(1, 15, 4).unwrap();
        assert_eq!(axis.offset_for_index(1, 4).unwrap(), 5);
        assert_eq!(axis.offset_for_index(2, 4).unwrap(), 20);
        assert_eq!(axis.total_extent(4).unwrap(), 40);

        axis.set_override(0, 10, 4).unwrap();
        assert!(!axis.is_override(0));
        assert_eq!(axis.offset_for_index(2, 4).unwrap(), 25);
        assert_eq!(axis.total_extent(4).unwrap(), 45);
    }

    #[test]
    fn variable_axis_reports_extent_overflow_instead_of_saturating() {
        let axis = VariableAxis::new(u32::MAX, 1, u32::MAX).unwrap();
        let error = axis
            .total_extent(2)
            .expect_err("u32 extent must be rejected");
        assert!(matches!(error, GridError::ExtentOverflow { .. }));
        assert_eq!(
            axis.total_extent_exact(2).unwrap(),
            u128::from(u32::MAX) * 2
        );
    }

    #[test]
    fn variable_axis_visible_interval_is_viewport_bounded_100k_rows() {
        let mut grid = make_grid(100_000, 1);
        grid.row_sizes
            .set_override(50_000, 999, 100_000)
            .expect("override");

        let viewport = Viewport::new(0.0, 0.0, 120.0, 200.0);
        let scroll = ScrollOffset::new(1_000_000.0, 0.0);
        let layout = grid.layout(viewport, scroll);

        assert_eq!(layout.content_height, 2_000_000 + (999 - 20));
        let Some(rows) = layout.body_rows else {
            panic!("expected visible row interval");
        };
        let expected_pixels = viewport.height + grid.overscan.rows;
        let max_rows = expected_pixels / grid.row_axis_default().min(1) + 2;
        assert!(rows.len() <= max_rows);
        assert!(rows.end < 100_000);
    }

    #[test]
    fn variable_axis_scrolling_respects_exact_boundaries() {
        let mut axis = VariableAxis::new(10, 1, 100).unwrap();
        axis.set_override(1, 20, 4).unwrap();
        assert_eq!(
            axis.visible_interval(0, 4, 10, 10, 0).unwrap(),
            Some(IndexRange { start: 1, end: 1 })
        );
        assert_eq!(
            axis.visible_interval(0, 4, 20, 10, 0).unwrap(),
            Some(IndexRange { start: 1, end: 2 })
        );
    }

    #[test]
    fn frozen_panes_subtract_space_and_hit_all_four_regions() {
        let mut grid = make_grid(20, 20);
        grid.set_frozen_panes(FrozenPanes::with(1, 1));
        let view = Viewport::new(100.0, 50.0, 60.0, 60.0);
        let scroll = ScrollOffset::new(12.0, 20.0);
        let layout = grid.layout(view, scroll);
        assert_eq!(layout.frozen_rows, Some(IndexRange { start: 0, end: 0 }));
        assert_eq!(layout.frozen_cols, Some(IndexRange { start: 0, end: 0 }));
        assert!(layout.body_rows.unwrap().start >= 1);
        assert!(layout.body_cols.unwrap().start >= 1);

        let hit = |x: f64, y: f64| grid.hit_test(Point::new(x, y), view, scroll);
        assert_eq!(
            hit(101.0, 51.0),
            Some(CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(0)))
        );
        assert_eq!(
            hit(113.0, 51.0),
            Some(CellAddress::new(SheetId(0), RowIndex(0), ColumnIndex(2)))
        );
        assert_eq!(
            hit(101.0, 71.0),
            Some(CellAddress::new(SheetId(0), RowIndex(2), ColumnIndex(0)))
        );
        assert_eq!(
            hit(113.0, 71.0),
            Some(CellAddress::new(SheetId(0), RowIndex(2), ColumnIndex(2)))
        );

        let boundary = grid.hit_test(Point::new(112.0, 70.0), view, ScrollOffset::default());
        assert_eq!(
            boundary,
            Some(CellAddress::new(SheetId(0), RowIndex(1), ColumnIndex(1)))
        );
    }

    #[test]
    fn cell_rect_is_document_coordinates_and_nonfinite_input_cannot_hit() {
        let mut grid = make_grid(30, 20);
        grid.set_frozen_panes(FrozenPanes::with(3, 2));
        let view = Viewport::new(0.0, 0.0, 120.0, 80.0);
        let rect = grid
            .cell_rect(CellAddress::new(SheetId(0), RowIndex(2), ColumnIndex(1)))
            .unwrap();
        assert_eq!(rect.x, 12.0);
        assert_eq!(rect.y, 40.0);
        assert_eq!(
            grid.hit_test(
                Point::new(rect.x + 1.0, rect.y + 1.0),
                view,
                ScrollOffset::default(),
            ),
            Some(CellAddress::new(SheetId(0), RowIndex(2), ColumnIndex(1)))
        );
        assert!(
            grid.hit_test(Point::new(f64::NAN, 1.0), view, ScrollOffset::default(),)
                .is_none()
        );
        assert!(
            grid.hit_test(
                Point::new(1.0, 1.0),
                Viewport::new(f64::NAN, 0.0, 120.0, 80.0),
                ScrollOffset::default(),
            )
            .is_none()
        );
    }

    #[test]
    fn finite_input_handlers_do_not_propagate_invalid_geometry() {
        let point = Point::new(f64::NAN, f64::NEG_INFINITY);
        assert!(!point.is_valid());
        assert!(point.x.is_finite() && point.y.is_finite());
        let rect = Rect::new(f64::INFINITY, -12.0, f64::NAN, 5.0);
        assert!(!rect.is_valid());
        assert!(rect.x.is_finite() && rect.y.is_finite() && rect.w.is_finite());
    }
}
