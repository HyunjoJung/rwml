//! Bounded flow placement across section pages and columns.

use super::paragraph_flow::{layout_paragraph, BodyFlowEntry};
use super::*;

/// Place an item at the current `y` on the last page, then advance `y`.
fn place_item(pages: &mut Pages, cursor: &mut FlowCursor, item: FlowItem, h: f32) {
    if let Some(p) = pages.last_mut() {
        p.push(PlacedItem {
            x: cursor.columns.x(cursor.column_index),
            width: cursor.columns.width(cursor.column_index),
            top: cursor.y,
            item,
        });
    }
    cursor.y += h;
    cursor.column_nonempty = true;
}

/// Break to a fresh page if `h` won't fit the remaining space on a non-empty page.
fn ensure(pages: &mut Pages, cursor: &mut FlowCursor, h: f32, geom: Geom) {
    if cursor.y + h > geom.bottom() && cursor.column_nonempty {
        cursor.advance(pages, geom);
    }
}

fn ensure_outside_top_bottom_bands(
    pages: &mut Pages,
    cursor: &mut FlowCursor,
    h: f32,
    geom: Geom,
    bands: &[ActiveTopBottomBand],
    ignored_owner: Option<usize>,
) {
    loop {
        ensure(pages, cursor, h, geom);
        let page_index = pages.len().saturating_sub(1);
        let adjusted_y = top_bottom_adjusted_y(cursor.y, h, page_index, bands, ignored_owner);
        if adjusted_y <= cursor.y {
            break;
        }
        cursor.y = adjusted_y;
    }
}

pub(super) fn top_bottom_adjusted_y(
    mut y: f32,
    h: f32,
    page_index: usize,
    bands: &[ActiveTopBottomBand],
    ignored_owner: Option<usize>,
) -> f32 {
    loop {
        let next_bottom = bands
            .iter()
            .filter(|band| {
                band.page_index == page_index
                    && match ignored_owner {
                        Some(owner) => band.owner_block != Some(owner),
                        None => true,
                    }
                    && y < band.bottom
                    && y + h > band.top
            })
            .map(|band| band.bottom)
            .max_by(f32::total_cmp);
        let Some(next_bottom) = next_bottom else {
            return y;
        };
        if next_bottom <= y {
            return y;
        }
        y = next_bottom;
    }
}

fn activate_reached_top_bottom_bands(
    pending: &mut Vec<PendingTopBottomBand>,
    active: &mut Vec<ActiveTopBottomBand>,
    deferred: &mut Vec<ActiveTopBottomBand>,
    defer_activation: bool,
    current_block: Option<usize>,
    line_range: Option<LineCharRange>,
    page_index: usize,
) {
    let Some(range) = line_range else {
        return;
    };
    let mut index = 0;
    while index < pending.len() {
        let band = pending[index];
        let reached = band.owner_block == current_block && range.contains(band.anchor_offset);
        if reached {
            pending.remove(index);
            if active.len() + deferred.len() < MAX_FLOATING_SHAPE_OVERLAYS {
                let reached_band = ActiveTopBottomBand {
                    owner_block: band.owner_block,
                    page_index,
                    top: band.top,
                    bottom: band.bottom,
                };
                if defer_activation {
                    deferred.push(reached_band);
                } else {
                    active.push(reached_band);
                }
            }
        } else {
            index += 1;
        }
    }
}

/// Re-place the header rows (clones) at the top of the current column.
fn repeat_headers(pages: &mut Pages, cursor: &mut FlowCursor, headers: &[RowLayout]) {
    for h in headers {
        let hr = h.clone();
        let hh = hr.height;
        place_item(pages, cursor, FlowItem::Row(hr), hh);
    }
}

pub(super) fn first_row_fragment_height(row: &RowLayout) -> f32 {
    row.cells
        .iter()
        .map(|cell| {
            let cut = (1..=cell.lines.len())
                .find(|cut| legal_cell_split(&cell.lines, *cut))
                .unwrap_or(0);
            cell.insets.top
                + cell
                    .lines
                    .iter()
                    .take(cut)
                    .map(LineLayout::cell_extent)
                    .sum::<f32>()
                + if cut == cell.lines.len() {
                    cell.insets.bottom
                } else {
                    0.0
                }
        })
        .fold(0.0_f32, f32::max)
        .max(14.0)
        .min(row.height)
}

/// Place one row, breaking pages as needed. A splittable row uses the remaining
/// column when it can hold a complete line. An authored `cantSplit` row that fits
/// a fresh column moves there whole; an over-tall row still splits at line
/// boundaries. `is_header` rows are never themselves preceded by a header repeat.
fn place_row(
    pages: &mut Pages,
    cursor: &mut FlowCursor,
    mut row: RowLayout,
    headers: &[RowLayout],
    is_header: bool,
    geom: Geom,
) -> usize {
    let mut on_fresh = !cursor.column_nonempty;
    let mut first_page = None;
    loop {
        let avail = geom.bottom() - cursor.y;
        if row.height <= avail {
            let h = row.height;
            place_item(pages, cursor, FlowItem::Row(row), h);
            let page = pages.len().saturating_sub(1);
            return *first_page.get_or_insert(page);
        }
        let remaining_can_hold_fragment = avail >= first_row_fragment_height(&row);
        if !on_fresh && (row.cant_split || !remaining_can_hold_fragment) {
            // Keep authored `cantSplit` rows together when they fit a fresh
            // column; also avoid forcing a partial line into a tiny remainder.
            cursor.advance(pages, geom);
            if !is_header {
                repeat_headers(pages, cursor, headers);
            }
            on_fresh = true;
            continue;
        }
        // On a fresh column (after any headers) and still too tall: split.
        let (frag, rest) = split_row(row, geom.bottom() - cursor.y);
        let fh = frag.height;
        place_item(pages, cursor, FlowItem::Row(frag), fh);
        let page = pages.len().saturating_sub(1);
        let table_first_page = *first_page.get_or_insert(page);
        match rest {
            Some(r) => {
                cursor.advance(pages, geom);
                if !is_header {
                    repeat_headers(pages, cursor, headers);
                }
                row = r;
                on_fresh = true;
            }
            None => return table_first_page,
        }
    }
}

/// Paginate a table: place every row, repeating the header rows after each break.
fn place_table(
    pages: &mut Pages,
    cursor: &mut FlowCursor,
    rows: Vec<RowLayout>,
    header_rows: usize,
    geom: Geom,
) -> Option<usize> {
    let mut headers: Vec<RowLayout> = rows.iter().take(header_rows).cloned().collect();
    // Only repeat headers that leave body space. A header that fills or exceeds the content box
    // would overflow or force a zero-height body fragment on every page. Dropping the repeat keeps
    // pagination linear; the header still renders inline once.
    let page_h = geom.bottom() - geom.top();
    if headers.iter().map(|h| h.height).sum::<f32>() >= page_h {
        headers.clear();
    }
    let mut first_page = None;
    for (i, row) in rows.into_iter().enumerate() {
        let page = place_row(pages, cursor, row, &headers, i < header_rows, geom);
        first_page.get_or_insert(page);
    }
    first_page
}

#[derive(Default)]
struct BlockProjectionState {
    block_pages: HashMap<usize, usize>,
    block_line_pages: HashMap<usize, Vec<BlockLinePage>>,
    block_line_widths: HashMap<usize, Vec<f32>>,
    pending_block: Option<usize>,
}

impl BlockProjectionState {
    fn mark_pending(&mut self, block_index: usize) {
        self.pending_block = Some(block_index);
    }

    fn record_pending_page(&mut self, page_index: usize) {
        if let Some(block_index) = self.pending_block.take() {
            self.block_pages.entry(block_index).or_insert(page_index);
        }
    }

    fn record_line(
        &mut self,
        current_block: Option<usize>,
        line: &LineLayout,
        page_index: usize,
        width: f32,
    ) {
        let Some(block_index) = current_block else {
            return;
        };
        if let Some(range) = line.char_range {
            self.block_line_pages
                .entry(block_index)
                .or_default()
                .push(BlockLinePage { page_index, range });
        }
        if width.is_finite() && width > 0.0 {
            self.block_line_widths
                .entry(block_index)
                .or_default()
                .push(width);
        }
    }
}

fn section_columns_by_item(items: &[FlowItem], final_columns: Option<u16>) -> Vec<Option<u16>> {
    let mut columns = vec![final_columns; items.len()];
    let mut section_start = 0usize;
    for (index, item) in items.iter().enumerate() {
        if let FlowItem::SectionBreak(setup) = item {
            columns[section_start..=index].fill(setup.columns);
            section_start = index + 1;
        }
    }
    columns
}

pub(super) fn section_column_gaps_by_item(
    items: &[FlowItem],
    final_column_gap_pt: Option<f32>,
) -> Vec<Option<f32>> {
    let mut gaps = vec![final_column_gap_pt; items.len()];
    let mut section_start = 0usize;
    let mut ending_gap = None;
    for (index, item) in items.iter().enumerate() {
        match item {
            FlowItem::SectionColumnGap(gap_pt) => ending_gap = Some(*gap_pt),
            FlowItem::SectionBreak(_) => {
                gaps[section_start..=index].fill(ending_gap);
                section_start = index + 1;
                ending_gap = None;
            }
            _ => {}
        }
    }
    gaps
}

pub(super) fn section_column_layouts_by_item(
    items: &[FlowItem],
    final_layout: Option<&SectionColumnLayoutHints>,
) -> Vec<Option<Rc<SectionColumnLayoutHints>>> {
    let mut layouts = vec![final_layout.cloned().map(Rc::new); items.len()];
    let mut section_start = 0usize;
    let mut ending_layout = None;
    for (index, item) in items.iter().enumerate() {
        match item {
            FlowItem::SectionColumnLayout(layout) => ending_layout = Some(Rc::clone(layout)),
            FlowItem::SectionBreak(_) => {
                layouts[section_start..=index].fill(ending_layout.clone());
                section_start = index + 1;
                ending_layout = None;
            }
            _ => {}
        }
    }
    layouts
}

pub(super) fn section_column_rtl_by_item(items: &[FlowItem], final_rtl: bool) -> Vec<bool> {
    let mut directions = vec![final_rtl; items.len()];
    let mut section_start = 0usize;
    let mut ending_rtl = false;
    for (index, item) in items.iter().enumerate() {
        match item {
            FlowItem::SectionColumnRtl => ending_rtl = true,
            FlowItem::SectionBreak(_) => {
                directions[section_start..=index].fill(ending_rtl);
                section_start = index + 1;
                ending_rtl = false;
            }
            _ => {}
        }
    }
    directions
}

fn same_section_column_layout(
    left: &Option<Rc<SectionColumnLayoutHints>>,
    right: &Option<Rc<SectionColumnLayoutHints>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Rc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq)]
struct BlockPaginationMetrics {
    pagination: PaginationHint,
    next_start: Option<usize>,
    line_heights: Vec<f32>,
    first_line_extent: f32,
    last_line_extent: f32,
    total_height: f32,
    is_paragraph: bool,
}

struct BlockPaginationMetricAccumulator {
    line_heights: Vec<f32>,
    extent: f32,
    first_line_extent: Option<f32>,
    last_line_extent: f32,
    is_paragraph: bool,
}

impl Default for BlockPaginationMetricAccumulator {
    fn default() -> Self {
        Self {
            line_heights: Vec::new(),
            extent: 0.0,
            first_line_extent: None,
            last_line_extent: 0.0,
            is_paragraph: true,
        }
    }
}

impl BlockPaginationMetricAccumulator {
    fn observe(&mut self, item: &FlowItem) {
        match item {
            FlowItem::Gap(height) => self.extent += height.max(0.0),
            FlowItem::Line(line) => {
                let height = line.height.max(0.0);
                self.extent += height;
                self.first_line_extent.get_or_insert(self.extent);
                self.last_line_extent = self.extent;
                self.line_heights.push(height);
            }
            FlowItem::BlockStart { .. } => unreachable!("block span excludes next anchor"),
            FlowItem::TopBottomBand { .. } => {}
            FlowItem::PaginationBoundary
            | FlowItem::Row(_)
            | FlowItem::PageBreak
            | FlowItem::ColumnBreak
            | FlowItem::SectionColumnGap(_)
            | FlowItem::SectionColumnLayout(_)
            | FlowItem::SectionColumnRtl
            | FlowItem::SectionBreak(_)
            | FlowItem::Table { .. }
            | FlowItem::Picture { .. }
            | FlowItem::Chart { .. } => self.is_paragraph = false,
        }
    }

    fn finish(
        self,
        pagination: PaginationHint,
        next_start: Option<usize>,
    ) -> BlockPaginationMetrics {
        BlockPaginationMetrics {
            pagination,
            next_start,
            first_line_extent: self.first_line_extent.unwrap_or(0.0),
            last_line_extent: self.last_line_extent,
            total_height: self.extent,
            is_paragraph: self.is_paragraph && !self.line_heights.is_empty(),
            line_heights: self.line_heights,
        }
    }
}

struct PendingBlockPaginationMetrics {
    start: usize,
    pagination: PaginationHint,
    metric: BlockPaginationMetricAccumulator,
}

#[derive(Default)]
struct StreamingBlockPaginationMetrics {
    active: Option<PendingBlockPaginationMetrics>,
    completed: Vec<(usize, BlockPaginationMetrics)>,
    item_count: usize,
}

impl StreamingBlockPaginationMetrics {
    fn observe(&mut self, item: &FlowItem) {
        let index = self.item_count;
        self.item_count = self.item_count.saturating_add(1);
        match item {
            FlowItem::BlockStart { pagination, .. } => {
                self.finish_active(Some(index));
                self.active = Some(PendingBlockPaginationMetrics {
                    start: index,
                    pagination: *pagination,
                    metric: BlockPaginationMetricAccumulator::default(),
                });
            }
            FlowItem::PaginationBoundary => self.finish_active(None),
            _ => {
                if let Some(active) = self.active.as_mut() {
                    active.metric.observe(item);
                }
            }
        }
    }

    fn finish(mut self) -> Vec<Option<BlockPaginationMetrics>> {
        self.finish_active(None);
        let mut metrics = vec![None; self.item_count];
        for (start, metric) in self.completed {
            metrics[start] = Some(metric);
        }
        metrics
    }

    fn finish_active(&mut self, next_start: Option<usize>) {
        let Some(active) = self.active.take() else {
            return;
        };
        self.completed.push((
            active.start,
            active.metric.finish(active.pagination, next_start),
        ));
    }
}

struct LoweredBodyFlow {
    items: Vec<FlowItem>,
    block_metrics: Vec<Option<BlockPaginationMetrics>>,
}

struct PaginationPlan {
    items: Vec<FlowItem>,
    columns_by_item: Vec<Option<u16>>,
    column_gaps_by_item: Vec<Option<f32>>,
    column_layouts_by_item: Vec<Option<Rc<SectionColumnLayoutHints>>>,
    column_rtl_by_item: Vec<bool>,
    geometries_by_item: Vec<Geom>,
    block_metrics: Vec<Option<BlockPaginationMetrics>>,
}

#[derive(Clone, Copy)]
struct PlannedItemContext<'a> {
    geom: Geom,
    columns: Option<u16>,
    column_gap_pt: Option<f32>,
    column_layout: &'a Option<Rc<SectionColumnLayoutHints>>,
    column_rtl: bool,
    block_metric: Option<&'a BlockPaginationMetrics>,
}

struct ActivePlacementTrack {
    geom: Geom,
    columns: Option<u16>,
    column_gap_pt: Option<f32>,
    column_layout: Option<Rc<SectionColumnLayoutHints>>,
    cursor: FlowCursor,
    column_rtl: bool,
}

impl ActivePlacementTrack {
    fn new(
        geom: Geom,
        columns: Option<u16>,
        column_gap_pt: Option<f32>,
        column_layout: Option<Rc<SectionColumnLayoutHints>>,
        column_rtl: bool,
    ) -> Self {
        let cursor = FlowCursor::new(
            geom,
            columns,
            column_gap_pt,
            column_layout.as_deref(),
            column_rtl,
        );
        Self {
            geom,
            columns,
            column_gap_pt,
            column_layout,
            cursor,
            column_rtl,
        }
    }

    fn synchronize(&mut self, context: PlannedItemContext<'_>) {
        if context.geom != self.geom {
            self.geom = context.geom;
            self.reset_columns(context);
        }
        if context.columns != self.columns
            || context.column_gap_pt != self.column_gap_pt
            || !same_section_column_layout(context.column_layout, &self.column_layout)
            || context.column_rtl != self.column_rtl
        {
            self.reset_columns(context);
        }
    }

    fn reset_columns(&mut self, context: PlannedItemContext<'_>) {
        self.cursor.set_columns(
            self.geom,
            context.columns,
            context.column_gap_pt,
            context.column_layout.as_deref(),
            context.column_rtl,
        );
        self.columns = context.columns;
        self.column_gap_pt = context.column_gap_pt;
        self.column_layout = context.column_layout.as_ref().map(Rc::clone);
        self.column_rtl = context.column_rtl;
    }
}

impl PaginationPlan {
    #[allow(clippy::too_many_arguments)]
    fn new(
        items: Vec<FlowItem>,
        supplied_block_metrics: Option<Vec<Option<BlockPaginationMetrics>>>,
        geom: Geom,
        final_section_setup: &SectionSetup,
        final_column_gap_pt: Option<f32>,
        final_column_layout: Option<&SectionColumnLayoutHints>,
        final_column_rtl: bool,
    ) -> Self {
        let columns_by_item = section_columns_by_item(&items, final_section_setup.columns);
        let column_gaps_by_item = section_column_gaps_by_item(&items, final_column_gap_pt);
        let column_layouts_by_item = section_column_layouts_by_item(&items, final_column_layout);
        let column_rtl_by_item = section_column_rtl_by_item(&items, final_column_rtl);
        let geometries_by_item = section_geometries_by_item(&items, geom);
        let block_metrics = supplied_block_metrics
            .filter(|metrics| metrics.len() == items.len())
            .unwrap_or_else(|| block_pagination_metrics(&items));
        Self {
            items,
            columns_by_item,
            column_gaps_by_item,
            column_layouts_by_item,
            column_rtl_by_item,
            geometries_by_item,
            block_metrics,
        }
    }

    fn item_context(&self, index: usize) -> Option<PlannedItemContext<'_>> {
        Some(PlannedItemContext {
            geom: *self.geometries_by_item.get(index)?,
            columns: *self.columns_by_item.get(index)?,
            column_gap_pt: *self.column_gaps_by_item.get(index)?,
            column_layout: self.column_layouts_by_item.get(index)?,
            column_rtl: *self.column_rtl_by_item.get(index)?,
            block_metric: self.block_metrics.get(index)?.as_ref(),
        })
    }
}

struct PlacementCoordinator {
    pages: Pages,
    page_sections: Vec<Option<RenderPageSection>>,
    section_start_page_index: usize,
    section_index: usize,
    active_track: ActivePlacementTrack,
    block_projection: BlockProjectionState,
    current_block: Option<usize>,
    current_block_start: Option<usize>,
    current_line_index: usize,
    widow_break_before: Option<usize>,
    pending_top_bottom_bands: Vec<PendingTopBottomBand>,
    active_top_bottom_bands: Vec<ActiveTopBottomBand>,
    deferred_top_bottom_bands: Vec<ActiveTopBottomBand>,
    previous_keep_next: bool,
    defer_current_top_bottom_bands: bool,
}

impl PlacementCoordinator {
    fn new(
        plan: &PaginationPlan,
        geom: Geom,
        final_section_setup: &SectionSetup,
        final_column_gap_pt: Option<f32>,
        final_column_rtl: bool,
    ) -> Self {
        let active_geom = plan.geometries_by_item.first().copied().unwrap_or(geom);
        let active_columns = plan
            .columns_by_item
            .first()
            .copied()
            .unwrap_or(final_section_setup.columns);
        let active_column_gap_pt = plan
            .column_gaps_by_item
            .first()
            .copied()
            .unwrap_or(final_column_gap_pt);
        let active_column_layout = plan.column_layouts_by_item.first().cloned().flatten();
        let active_column_rtl = plan
            .column_rtl_by_item
            .first()
            .copied()
            .unwrap_or(final_column_rtl);
        let active_track = ActivePlacementTrack::new(
            active_geom,
            active_columns,
            active_column_gap_pt,
            active_column_layout,
            active_column_rtl,
        );
        Self {
            pages: vec![Vec::new()],
            page_sections: vec![None],
            section_start_page_index: 0,
            section_index: 0,
            active_track,
            block_projection: BlockProjectionState::default(),
            current_block: None,
            current_block_start: None,
            current_line_index: 0,
            widow_break_before: None,
            pending_top_bottom_bands: Vec::new(),
            active_top_bottom_bands: Vec::new(),
            deferred_top_bottom_bands: Vec::new(),
            previous_keep_next: false,
            defer_current_top_bottom_bands: false,
        }
    }
}

fn block_pagination_metrics(items: &[FlowItem]) -> Vec<Option<BlockPaginationMetrics>> {
    let starts = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            FlowItem::BlockStart { .. } => Some(index),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut metrics = vec![None; items.len()];
    for (position, &start) in starts.iter().enumerate() {
        let candidate_next = starts.get(position + 1).copied();
        let scan_end = candidate_next.unwrap_or(items.len());
        let boundary = items[start + 1..scan_end]
            .iter()
            .position(|item| matches!(item, FlowItem::PaginationBoundary))
            .map(|offset| start + 1 + offset);
        let end = boundary.unwrap_or(scan_end);
        let next_start = if boundary.is_some() {
            None
        } else {
            candidate_next
        };
        let pagination = match items[start] {
            FlowItem::BlockStart { pagination, .. } => pagination,
            _ => PaginationHint::default(),
        };
        let mut metric = BlockPaginationMetricAccumulator::default();
        for item in &items[start + 1..end] {
            metric.observe(item);
        }
        metrics[start] = Some(metric.finish(pagination, next_start));
    }
    metrics
}

fn keep_next_chain_height(
    start: usize,
    metrics: &[Option<BlockPaginationMetrics>],
    columns_by_item: &[Option<u16>],
) -> Option<f32> {
    const MAX_KEEP_NEXT_CHAIN: usize = 32;

    let chain_columns = columns_by_item.get(start).copied().flatten();
    let mut current = start;
    let mut height = 0.0;
    for _ in 0..MAX_KEEP_NEXT_CHAIN {
        let metric = metrics.get(current)?.as_ref()?;
        if !metric.is_paragraph || !metric.pagination.keep_next {
            return None;
        }
        height += metric.total_height;
        let next = metric.next_start?;
        if columns_by_item.get(next).copied().flatten() != chain_columns {
            return None;
        }
        let next_metric = metrics.get(next)?.as_ref()?;
        if !next_metric.is_paragraph {
            return None;
        }
        if next_metric.pagination.keep_next {
            current = next;
        } else {
            return Some(height + next_metric.first_line_extent);
        }
    }
    None
}

fn fitting_line_count_with_bands(
    line_heights: &[f32],
    mut y: f32,
    page_index: usize,
    geom: Geom,
    bands: &[ActiveTopBottomBand],
) -> usize {
    let mut count = 0;
    for &height in line_heights {
        y = top_bottom_adjusted_y(y, height, page_index, bands, None);
        if y + height > geom.bottom() + f32::EPSILON {
            break;
        }
        y += height;
        count += 1;
    }
    count
}

fn move_to_fresh_column_for_required_height(
    pages: &mut Pages,
    cursor: &mut FlowCursor,
    required_height: f32,
    geom: Geom,
    bands: &[ActiveTopBottomBand],
) {
    let body_height = geom.bottom() - geom.top();
    if required_height > body_height {
        if cursor.column_nonempty {
            cursor.advance(pages, geom);
        }
        return;
    }
    loop {
        let page_index = pages.len().saturating_sub(1);
        let adjusted_y = top_bottom_adjusted_y(cursor.y, required_height, page_index, bands, None);
        if adjusted_y + required_height <= geom.bottom() + f32::EPSILON {
            cursor.y = adjusted_y;
            return;
        }
        cursor.advance(pages, geom);
    }
}

fn page_after_section_break(current_page: usize, section_break: Option<SectionBreakKind>) -> usize {
    let next_page = current_page + 1;
    match section_break.unwrap_or(SectionBreakKind::NextPage) {
        SectionBreakKind::EvenPage if next_page % 2 == 1 => next_page + 1,
        SectionBreakKind::OddPage if next_page % 2 == 0 => next_page + 1,
        SectionBreakKind::NextPage | SectionBreakKind::EvenPage | SectionBreakKind::OddPage => {
            next_page
        }
    }
}

#[cfg(test)]
pub(super) fn paginate(
    items: Vec<FlowItem>,
    geom: Geom,
    final_section_setup: &SectionSetup,
) -> Pagination {
    paginate_with_column_gap(items, geom, final_section_setup, None, None, false)
}

#[cfg(test)]
pub(super) fn paginate_with_column_gap(
    items: Vec<FlowItem>,
    geom: Geom,
    final_section_setup: &SectionSetup,
    final_column_gap_pt: Option<f32>,
    final_column_layout: Option<&SectionColumnLayoutHints>,
    final_column_rtl: bool,
) -> Pagination {
    paginate_with_column_gap_and_metrics(
        items,
        None,
        geom,
        final_section_setup,
        final_column_gap_pt,
        final_column_layout,
        final_column_rtl,
    )
}

#[allow(clippy::too_many_arguments)]
fn paginate_with_column_gap_and_metrics(
    items: Vec<FlowItem>,
    supplied_block_metrics: Option<Vec<Option<BlockPaginationMetrics>>>,
    geom: Geom,
    final_section_setup: &SectionSetup,
    final_column_gap_pt: Option<f32>,
    final_column_layout: Option<&SectionColumnLayoutHints>,
    final_column_rtl: bool,
) -> Pagination {
    let plan = PaginationPlan::new(
        items,
        supplied_block_metrics,
        geom,
        final_section_setup,
        final_column_gap_pt,
        final_column_layout,
        final_column_rtl,
    );
    paginate_plan(
        plan,
        geom,
        final_section_setup,
        final_column_gap_pt,
        final_column_rtl,
    )
}

fn paginate_plan(
    plan: PaginationPlan,
    geom: Geom,
    final_section_setup: &SectionSetup,
    final_column_gap_pt: Option<f32>,
    final_column_rtl: bool,
) -> Pagination {
    let state = PlacementCoordinator::new(
        &plan,
        geom,
        final_section_setup,
        final_column_gap_pt,
        final_column_rtl,
    );
    state.paginate(plan, final_section_setup)
}

impl PlacementCoordinator {
    fn paginate(self, plan: PaginationPlan, final_section_setup: &SectionSetup) -> Pagination {
        paginate_with_state(self, plan, final_section_setup)
    }
}

fn paginate_with_state(
    state: PlacementCoordinator,
    mut plan: PaginationPlan,
    final_section_setup: &SectionSetup,
) -> Pagination {
    // Paginate flow items top-to-bottom through section columns and then across
    // pages. Tables repeat headers after each break and split oversized rows.
    let items = std::mem::take(&mut plan.items);
    let PlacementCoordinator {
        mut pages,
        mut page_sections,
        mut section_start_page_index,
        mut section_index,
        mut active_track,
        mut block_projection,
        mut current_block,
        mut current_block_start,
        mut current_line_index,
        mut widow_break_before,
        mut pending_top_bottom_bands,
        mut active_top_bottom_bands,
        mut deferred_top_bottom_bands,
        mut previous_keep_next,
        mut defer_current_top_bottom_bands,
    } = state;
    for (item_index, item) in items.into_iter().enumerate() {
        let context = plan
            .item_context(item_index)
            .expect("pagination plan keeps item contexts aligned");
        active_track.synchronize(context);
        let active_geom = active_track.geom;
        let cursor = &mut active_track.cursor;
        match item {
            FlowItem::BlockStart {
                index: block_index,
                pagination,
            } => {
                let protected_by_previous_keep = previous_keep_next;
                if !protected_by_previous_keep {
                    active_top_bottom_bands.append(&mut deferred_top_bottom_bands);
                }
                previous_keep_next = pagination.keep_next;
                defer_current_top_bottom_bands = protected_by_previous_keep
                    || pagination.keep_next
                    || pagination.keep_lines
                    || pagination.widow_control;
                pending_top_bottom_bands.clear();
                block_projection.record_pending_page(pages.len().saturating_sub(1));
                if let Some(metric) = context.block_metric {
                    if pagination.keep_next {
                        if let Some(height) = keep_next_chain_height(
                            item_index,
                            &plan.block_metrics,
                            &plan.columns_by_item,
                        ) {
                            move_to_fresh_column_for_required_height(
                                &mut pages,
                                cursor,
                                height,
                                active_geom,
                                &active_top_bottom_bands,
                            );
                        }
                    }
                    let keep_whole_paragraph = pagination.keep_lines
                        || (pagination.widow_control
                            && metric.line_heights.len() <= 3
                            && metric.last_line_extent <= active_geom.bottom() - active_geom.top());
                    if keep_whole_paragraph {
                        move_to_fresh_column_for_required_height(
                            &mut pages,
                            cursor,
                            metric.last_line_extent,
                            active_geom,
                            &active_top_bottom_bands,
                        );
                    }
                }
                block_projection.mark_pending(block_index);
                current_block = Some(block_index);
                current_block_start = Some(item_index);
                current_line_index = 0;
                widow_break_before = None;
            }
            FlowItem::PaginationBoundary => {
                block_projection.record_pending_page(pages.len().saturating_sub(1));
                current_block = None;
                current_block_start = None;
                current_line_index = 0;
                widow_break_before = None;
                pending_top_bottom_bands.clear();
                active_top_bottom_bands.clear();
                deferred_top_bottom_bands.clear();
                previous_keep_next = false;
                defer_current_top_bottom_bands = false;
            }
            FlowItem::TopBottomBand {
                top,
                bottom,
                anchor_offset,
            } => {
                if top < bottom && pending_top_bottom_bands.len() < MAX_FLOATING_SHAPE_OVERLAYS {
                    pending_top_bottom_bands.push(PendingTopBottomBand {
                        owner_block: current_block,
                        anchor_offset,
                        top: top.max(active_geom.top()),
                        bottom: bottom.min(active_geom.bottom()),
                    });
                }
            }
            FlowItem::Gap(g) => cursor.y += g,
            FlowItem::Line(l) => {
                let h = l.height;
                ensure_outside_top_bottom_bands(
                    &mut pages,
                    cursor,
                    h,
                    active_geom,
                    &active_top_bottom_bands,
                    None,
                );
                if let Some(metric) = current_block_start
                    .and_then(|start| plan.block_metrics.get(start))
                    .and_then(Option::as_ref)
                    .filter(|metric| metric.pagination.widow_control)
                {
                    loop {
                        if widow_break_before == Some(current_line_index) {
                            cursor.advance(&mut pages, active_geom);
                            widow_break_before = None;
                            continue;
                        }
                        if widow_break_before.is_none()
                            && current_line_index < metric.line_heights.len()
                        {
                            let remaining = metric.line_heights.len() - current_line_index;
                            let fits = fitting_line_count_with_bands(
                                &metric.line_heights[current_line_index..],
                                cursor.y,
                                pages.len().saturating_sub(1),
                                active_geom,
                                &active_top_bottom_bands,
                            );
                            if fits < remaining {
                                if fits < 2 && cursor.column_nonempty {
                                    cursor.advance(&mut pages, active_geom);
                                    continue;
                                }
                                if remaining - fits == 1 {
                                    let bottom_lines = fits.saturating_sub(1);
                                    if bottom_lines >= 2 {
                                        widow_break_before =
                                            Some(current_line_index + bottom_lines);
                                    } else {
                                        let remaining_height = metric.line_heights
                                            [current_line_index..]
                                            .iter()
                                            .sum::<f32>();
                                        if cursor.column_nonempty
                                            && remaining_height
                                                <= active_geom.bottom() - active_geom.top()
                                        {
                                            cursor.advance(&mut pages, active_geom);
                                            continue;
                                        }
                                    }
                                }
                            }
                        }
                        break;
                    }
                }
                ensure_outside_top_bottom_bands(
                    &mut pages,
                    cursor,
                    h,
                    active_geom,
                    &active_top_bottom_bands,
                    None,
                );
                let page_index = pages.len().saturating_sub(1);
                block_projection.record_pending_page(page_index);
                block_projection.record_line(
                    current_block,
                    &l,
                    page_index,
                    cursor.columns.width(cursor.column_index),
                );
                let line_range = l.char_range;
                place_item(&mut pages, cursor, FlowItem::Line(l), h);
                activate_reached_top_bottom_bands(
                    &mut pending_top_bottom_bands,
                    &mut active_top_bottom_bands,
                    &mut deferred_top_bottom_bands,
                    defer_current_top_bottom_bands,
                    current_block,
                    line_range,
                    page_index,
                );
                current_line_index = current_line_index.saturating_add(1);
            }
            FlowItem::Picture { image, layout } => {
                ensure_outside_top_bottom_bands(
                    &mut pages,
                    cursor,
                    layout.bounds_h,
                    active_geom,
                    &active_top_bottom_bands,
                    current_block,
                );
                block_projection.record_pending_page(pages.len().saturating_sub(1));
                place_item(
                    &mut pages,
                    cursor,
                    FlowItem::Picture { image, layout },
                    layout.bounds_h,
                );
            }
            FlowItem::Chart { chart, w, h } => {
                ensure_outside_top_bottom_bands(
                    &mut pages,
                    cursor,
                    h,
                    active_geom,
                    &active_top_bottom_bands,
                    None,
                );
                block_projection.record_pending_page(pages.len().saturating_sub(1));
                place_item(&mut pages, cursor, FlowItem::Chart { chart, w, h }, h);
            }
            FlowItem::Table { rows, header_rows } => {
                let fallback_page = pages.len().saturating_sub(1);
                let first_page = place_table(&mut pages, cursor, rows, header_rows, active_geom)
                    .unwrap_or(fallback_page);
                block_projection.record_pending_page(first_page);
            }
            FlowItem::PageBreak => {
                cursor.force_page(&mut pages, active_geom);
                block_projection.record_pending_page(pages.len().saturating_sub(1));
            }
            FlowItem::ColumnBreak => {
                cursor.advance(&mut pages, active_geom);
                block_projection.record_pending_page(pages.len().saturating_sub(1));
            }
            FlowItem::SectionColumnGap(_) => {}
            FlowItem::SectionColumnLayout(_) => {}
            FlowItem::SectionColumnRtl => {}
            FlowItem::SectionBreak(section) => {
                let next_section_page =
                    page_after_section_break(pages.len(), section.section_break);
                while pages.len() < next_section_page {
                    cursor.force_page(&mut pages, active_geom);
                }
                page_sections.resize(pages.len(), None);
                assign_section_to_render_pages(
                    &mut page_sections,
                    section_start_page_index,
                    next_section_page.saturating_sub(2),
                    &section,
                    section_index,
                );
                block_projection.record_pending_page(pages.len().saturating_sub(1));
                section_start_page_index = next_section_page.saturating_sub(1);
                section_index = section_index.saturating_add(1);
            }
            // Rows reach pagination only inside a Table; place defensively.
            FlowItem::Row(r) => {
                let h = r.height;
                ensure_outside_top_bottom_bands(
                    &mut pages,
                    cursor,
                    h,
                    active_geom,
                    &active_top_bottom_bands,
                    None,
                );
                block_projection.record_pending_page(pages.len().saturating_sub(1));
                place_item(&mut pages, cursor, FlowItem::Row(r), h);
            }
        }
    }
    block_projection.record_pending_page(pages.len().saturating_sub(1));
    page_sections.resize(pages.len(), None);
    assign_section_to_render_pages(
        &mut page_sections,
        section_start_page_index,
        pages.len().saturating_sub(1),
        final_section_setup,
        section_index,
    );
    let BlockProjectionState {
        block_pages,
        block_line_pages,
        block_line_widths,
        ..
    } = block_projection;
    Pagination {
        pages,
        page_sections,
        block_pages,
        block_line_pages,
        block_line_widths,
        final_section_start_page_index: section_start_page_index,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paginate_body_flow_with_column_gap(
    flow: BodyFlowQueue<'_>,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    geom: Geom,
    final_section_setup: &SectionSetup,
    final_column_gap_pt: Option<f32>,
    final_column_layout: Option<&SectionColumnLayoutHints>,
    final_column_rtl: bool,
) -> Pagination {
    let lowered = lower_body_flow_entries_with_metrics(flow, cx, capture);
    #[cfg(test)]
    assert_eq!(
        lowered.block_metrics,
        block_pagination_metrics(&lowered.items)
    );
    paginate_with_column_gap_and_metrics(
        lowered.items,
        Some(lowered.block_metrics),
        geom,
        final_section_setup,
        final_column_gap_pt,
        final_column_layout,
        final_column_rtl,
    )
}

fn lower_body_flow_entries_with_metrics(
    flow: BodyFlowQueue<'_>,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) -> LoweredBodyFlow {
    let mut items = Vec::with_capacity(flow.ready_item_count());
    let mut block_metrics = StreamingBlockPaginationMetrics::default();
    for entry in flow.into_entries() {
        match entry {
            BodyFlowEntry::Ready(item) => {
                block_metrics.observe(&item);
                items.push(item);
            }
            BodyFlowEntry::Paragraph(request) => {
                let start = items.len();
                layout_paragraph(request, &mut items, cx, capture);
                for item in &items[start..] {
                    block_metrics.observe(item);
                }
            }
        }
    }
    LoweredBodyFlow {
        items,
        block_metrics: block_metrics.finish(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
    use parley::{FontContext, LayoutContext};

    use super::*;

    type MetricSnapshot = (PaginationHint, Option<usize>, Vec<u32>, u32, u32, u32, bool);

    fn strict_font_context(font: Vec<u8>) -> FontContext {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        collection.register_fonts(Blob::from(font), None);
        FontContext {
            collection,
            source_cache: SourceCache::default(),
        }
    }

    fn page_field_paragraph(text: &str) -> Paragraph {
        Paragraph {
            runs: vec![Run {
                text: text.to_string(),
                field: FieldRole::Simple {
                    instruction: "PAGE".to_string(),
                },
                ..Run::default()
            }],
            ..Paragraph::default()
        }
    }

    fn metric_snapshots(metrics: &[Option<BlockPaginationMetrics>]) -> Vec<Option<MetricSnapshot>> {
        metrics
            .iter()
            .map(|metric| {
                metric.as_ref().map(|metric| {
                    (
                        metric.pagination,
                        metric.next_start,
                        metric
                            .line_heights
                            .iter()
                            .map(|height| height.to_bits())
                            .collect(),
                        metric.first_line_extent.to_bits(),
                        metric.last_line_extent.to_bits(),
                        metric.total_height.to_bits(),
                        metric.is_paragraph,
                    )
                })
            })
            .collect()
    }

    fn dynamic_page_field_indices(items: &[FlowItem]) -> Vec<Option<usize>> {
        items
            .iter()
            .filter_map(|item| match item {
                FlowItem::Line(line) => Some(line),
                _ => None,
            })
            .flat_map(|line| &line.runs)
            .filter_map(|run| run.dynamic.as_ref())
            .map(|dynamic| dynamic.page_field_index)
            .collect()
    }

    fn line(height: f32) -> FlowItem {
        FlowItem::Line(LineLayout {
            height,
            baseline: height * 0.8,
            clip_to_height: false,
            x_indent: 0.0,
            char_range: None,
            background: None,
            cell_spacing: CellLineSpacing::default(),
            cell_paragraph: None,
            cell_cant_split_group: None,
            cell_visual: None,
            leaders: Vec::new(),
            runs: Vec::new(),
        })
    }

    fn page_line_counts(pagination: &Pagination) -> Vec<usize> {
        pagination
            .pages
            .iter()
            .map(|page| {
                page.iter()
                    .filter(|placed| matches!(placed.item, FlowItem::Line(_)))
                    .count()
            })
            .collect()
    }

    #[test]
    fn block_metric_accumulator_preserves_paragraph_extent_semantics() {
        let pagination = PaginationHint {
            keep_next: true,
            widow_control: true,
            ..PaginationHint::default()
        };
        let mut metric = BlockPaginationMetricAccumulator::default();
        for item in [
            FlowItem::Gap(5.0),
            FlowItem::TopBottomBand {
                top: 10.0,
                bottom: 20.0,
                anchor_offset: 0,
            },
            line(10.0),
            line(12.0),
            FlowItem::Gap(7.0),
        ] {
            metric.observe(&item);
        }
        let metric = metric.finish(pagination, Some(8));

        assert_eq!(metric.pagination, pagination);
        assert_eq!(metric.next_start, Some(8));
        assert_eq!(metric.line_heights, vec![10.0, 12.0]);
        assert_eq!(metric.first_line_extent, 15.0);
        assert_eq!(metric.last_line_extent, 27.0);
        assert_eq!(metric.total_height, 34.0);
        assert!(metric.is_paragraph);

        let mut controlled = BlockPaginationMetricAccumulator::default();
        for item in [
            FlowItem::Gap(3.0),
            line(9.0),
            FlowItem::ColumnBreak,
            FlowItem::Gap(4.0),
        ] {
            controlled.observe(&item);
        }
        let controlled = controlled.finish(PaginationHint::default(), None);
        assert_eq!(controlled.first_line_extent, 12.0);
        assert_eq!(controlled.last_line_extent, 12.0);
        assert_eq!(controlled.total_height, 16.0);
        assert!(!controlled.is_paragraph);

        let mut empty = BlockPaginationMetricAccumulator::default();
        empty.observe(&FlowItem::Gap(6.0));
        let empty = empty.finish(PaginationHint::default(), None);
        assert!(empty.line_heights.is_empty());
        assert_eq!(empty.total_height, 6.0);
        assert!(!empty.is_paragraph);
    }

    #[test]
    fn block_metric_scan_stops_at_pagination_boundary() {
        let items = vec![
            FlowItem::BlockStart {
                index: 0,
                pagination: PaginationHint::default(),
            },
            FlowItem::Gap(2.0),
            line(10.0),
            FlowItem::PaginationBoundary,
            FlowItem::Gap(99.0),
            FlowItem::BlockStart {
                index: 1,
                pagination: PaginationHint::default(),
            },
            line(8.0),
        ];
        let metrics = block_pagination_metrics(&items);
        let first = metrics[0].as_ref().expect("first block metric");

        assert_eq!(first.next_start, None);
        assert_eq!(first.line_heights, vec![10.0]);
        assert_eq!(first.first_line_extent, 12.0);
        assert_eq!(first.last_line_extent, 12.0);
        assert_eq!(first.total_height, 12.0);
        assert!(first.is_paragraph);
        assert!(metrics[5].is_some());
    }

    #[test]
    fn metric_bearing_paginator_uses_supplied_block_metrics() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let items = vec![
            FlowItem::BlockStart {
                index: 0,
                pagination: PaginationHint::default(),
            },
            line(40.0),
            FlowItem::BlockStart {
                index: 1,
                pagination: PaginationHint {
                    keep_lines: true,
                    ..PaginationHint::default()
                },
            },
            line(10.0),
            line(10.0),
            line(10.0),
        ];
        let mut supplied = block_pagination_metrics(&items);
        supplied[2]
            .as_mut()
            .expect("protected paragraph metric")
            .last_line_extent = 0.0;

        let pagination = paginate_with_column_gap_and_metrics(
            items,
            Some(supplied),
            geom,
            &SectionSetup::default(),
            None,
            None,
            false,
        );

        assert_eq!(page_line_counts(&pagination), vec![3, 1]);
    }

    #[test]
    fn pagination_plan_keeps_section_sidecars_and_metrics_item_aligned() {
        let ending = SectionSetup {
            columns: Some(2),
            page: PageSetup {
                width_pt: 240.0,
                height_pt: 320.0,
                margin_pt: 20.0,
                ..PageSetup::default()
            },
            ..SectionSetup::default()
        };
        let final_section = SectionSetup {
            columns: Some(1),
            ..SectionSetup::default()
        };
        let geom = Geom::from_section(&final_section);
        let items = vec![
            FlowItem::BlockStart {
                index: 0,
                pagination: PaginationHint::default(),
            },
            line(10.0),
            FlowItem::SectionColumnGap(8.0),
            FlowItem::SectionColumnRtl,
            FlowItem::SectionBreak(ending.clone()),
            FlowItem::BlockStart {
                index: 1,
                pagination: PaginationHint::default(),
            },
            line(12.0),
        ];
        let metrics = block_pagination_metrics(&items);
        let plan = PaginationPlan::new(
            items,
            Some(metrics),
            geom,
            &final_section,
            Some(5.0),
            None,
            false,
        );

        assert_eq!(plan.items.len(), 7);
        assert_eq!(plan.columns_by_item.len(), plan.items.len());
        assert_eq!(plan.column_gaps_by_item.len(), plan.items.len());
        assert_eq!(plan.column_layouts_by_item.len(), plan.items.len());
        assert_eq!(plan.column_rtl_by_item.len(), plan.items.len());
        assert_eq!(plan.geometries_by_item.len(), plan.items.len());
        assert_eq!(plan.block_metrics.len(), plan.items.len());
        assert!(plan.columns_by_item[..=4]
            .iter()
            .all(|columns| *columns == Some(2)));
        assert!(plan.columns_by_item[5..]
            .iter()
            .all(|columns| *columns == Some(1)));
        assert!(plan.column_gaps_by_item[..=4]
            .iter()
            .all(|gap| *gap == Some(8.0)));
        assert!(plan.column_gaps_by_item[5..]
            .iter()
            .all(|gap| *gap == Some(5.0)));
        assert!(plan.column_rtl_by_item[..=4].iter().all(|rtl| *rtl));
        assert!(plan.column_rtl_by_item[5..].iter().all(|rtl| !rtl));
        assert!(plan.geometries_by_item[..=4]
            .iter()
            .all(|item_geom| *item_geom == Geom::from_section(&ending)));
        assert!(plan.geometries_by_item[5..]
            .iter()
            .all(|item_geom| *item_geom == geom));

        let fallback = PaginationPlan::new(
            vec![
                FlowItem::BlockStart {
                    index: 0,
                    pagination: PaginationHint::default(),
                },
                line(9.0),
            ],
            Some(Vec::new()),
            geom,
            &final_section,
            None,
            None,
            false,
        );
        assert_eq!(fallback.block_metrics.len(), fallback.items.len());
        assert!(fallback.block_metrics[0].is_some());
    }

    #[test]
    fn pagination_plan_exposes_checked_item_contexts() {
        let ending_layout = Rc::new(SectionColumnLayoutHints::default());
        let final_layout = SectionColumnLayoutHints::default();
        let ending = SectionSetup {
            columns: Some(2),
            page: PageSetup {
                width_pt: 240.0,
                height_pt: 320.0,
                margin_pt: 20.0,
                ..PageSetup::default()
            },
            ..SectionSetup::default()
        };
        let final_section = SectionSetup {
            columns: Some(1),
            ..SectionSetup::default()
        };
        let geom = Geom::from_section(&final_section);
        let items = vec![
            FlowItem::BlockStart {
                index: 0,
                pagination: PaginationHint::default(),
            },
            FlowItem::SectionColumnLayout(Rc::clone(&ending_layout)),
            FlowItem::SectionColumnGap(8.0),
            FlowItem::SectionColumnRtl,
            FlowItem::SectionBreak(ending.clone()),
            FlowItem::BlockStart {
                index: 1,
                pagination: PaginationHint::default(),
            },
            line(12.0),
        ];
        let metrics = block_pagination_metrics(&items);
        let plan = PaginationPlan::new(
            items,
            Some(metrics),
            geom,
            &final_section,
            Some(5.0),
            Some(&final_layout),
            false,
        );

        let ending_context = plan.item_context(0).expect("ending-section context");
        assert!(ending_context.geom == Geom::from_section(&ending));
        assert_eq!(ending_context.columns, Some(2));
        assert_eq!(ending_context.column_gap_pt, Some(8.0));
        assert!(Rc::ptr_eq(
            ending_context
                .column_layout
                .as_ref()
                .expect("ending layout"),
            &ending_layout,
        ));
        assert!(ending_context.column_rtl);
        assert!(ending_context.block_metric.is_some());

        let final_context = plan.item_context(5).expect("final-section context");
        assert!(final_context.geom == geom);
        assert_eq!(final_context.columns, Some(1));
        assert_eq!(final_context.column_gap_pt, Some(5.0));
        assert_eq!(
            final_context
                .column_layout
                .as_deref()
                .expect("final layout"),
            &final_layout,
        );
        assert!(!final_context.column_rtl);
        assert!(final_context.block_metric.is_some());
        assert!(plan.item_context(plan.items.len()).is_none());
    }

    #[test]
    fn pagination_plan_consumer_matches_the_vector_entry() {
        let ending = SectionSetup {
            section_break: Some(SectionBreakKind::NextPage),
            columns: Some(2),
            ..SectionSetup::default()
        };
        let final_section = SectionSetup {
            columns: Some(1),
            ..SectionSetup::default()
        };
        let geom = Geom::from_section(&final_section);
        let items = || {
            vec![
                FlowItem::BlockStart {
                    index: 0,
                    pagination: PaginationHint::default(),
                },
                line(10.0),
                FlowItem::SectionColumnGap(8.0),
                FlowItem::SectionColumnRtl,
                FlowItem::SectionBreak(ending.clone()),
                FlowItem::BlockStart {
                    index: 1,
                    pagination: PaginationHint::default(),
                },
                line(12.0),
            ]
        };
        let plan_items = items();
        let plan_metrics = block_pagination_metrics(&plan_items);
        let plan = PaginationPlan::new(
            plan_items,
            Some(plan_metrics),
            geom,
            &final_section,
            Some(5.0),
            None,
            false,
        );

        let planned = paginate_plan(plan, geom, &final_section, Some(5.0), false);
        let vector =
            paginate_with_column_gap(items(), geom, &final_section, Some(5.0), None, false);
        let section_snapshot = |pagination: &Pagination| {
            pagination
                .page_sections
                .iter()
                .map(|section| {
                    section.as_ref().map(|section| {
                        (
                            section.first_page_index,
                            section.section_index,
                            section.setup.clone(),
                        )
                    })
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(page_line_counts(&planned), page_line_counts(&vector));
        assert_eq!(planned.block_pages, vector.block_pages);
        assert_eq!(
            planned.final_section_start_page_index,
            vector.final_section_start_page_index
        );
        assert_eq!(section_snapshot(&planned), section_snapshot(&vector));
    }

    #[test]
    fn placement_coordinator_initializes_from_the_first_planned_track() {
        let ending = SectionSetup {
            columns: Some(2),
            ..SectionSetup::default()
        };
        let final_section = SectionSetup {
            columns: Some(1),
            ..SectionSetup::default()
        };
        let geom = Geom::from_section(&final_section);
        let items = vec![
            FlowItem::BlockStart {
                index: 0,
                pagination: PaginationHint::default(),
            },
            line(10.0),
            FlowItem::SectionColumnGap(8.0),
            FlowItem::SectionColumnRtl,
            FlowItem::SectionBreak(ending.clone()),
        ];
        let metrics = block_pagination_metrics(&items);
        let plan = PaginationPlan::new(
            items,
            Some(metrics),
            geom,
            &final_section,
            Some(5.0),
            None,
            false,
        );

        let state = PlacementCoordinator::new(&plan, geom, &final_section, Some(5.0), false);

        assert_eq!(state.pages.len(), 1);
        assert_eq!(state.page_sections.len(), 1);
        assert!(state.page_sections[0].is_none());
        assert!(state.active_track.geom == Geom::from_section(&ending));
        assert_eq!(state.active_track.columns, Some(2));
        assert_eq!(state.active_track.column_gap_pt, Some(8.0));
        assert!(state.active_track.column_rtl);
        assert_eq!(state.active_track.cursor.columns.count, 2);
        assert_eq!(state.active_track.cursor.y, state.active_track.geom.top());
        assert!(state.block_projection.block_pages.is_empty());
        assert!(state.block_projection.block_line_pages.is_empty());
        assert!(state.block_projection.block_line_widths.is_empty());
    }

    #[test]
    fn active_placement_track_synchronizes_geometry_and_column_changes() {
        let initial_geom = Geom::from_section(&SectionSetup::default());
        let initial_layout = Some(Rc::new(SectionColumnLayoutHints::default()));
        let mut track =
            ActivePlacementTrack::new(initial_geom, Some(2), Some(8.0), initial_layout, true);
        let changed_geom = Geom::from_section(&SectionSetup {
            page: PageSetup {
                width_pt: 240.0,
                height_pt: 320.0,
                margin_pt: 20.0,
                ..PageSetup::default()
            },
            ..SectionSetup::default()
        });
        let geometry_layout = Some(Rc::new(SectionColumnLayoutHints::default()));
        track.cursor.y += 24.0;
        track.cursor.column_nonempty = true;

        track.synchronize(PlannedItemContext {
            geom: changed_geom,
            columns: Some(1),
            column_gap_pt: Some(5.0),
            column_layout: &geometry_layout,
            column_rtl: false,
            block_metric: None,
        });

        assert!(track.geom == changed_geom);
        assert_eq!(track.columns, Some(1));
        assert_eq!(track.column_gap_pt, Some(5.0));
        assert!(!track.column_rtl);
        assert_eq!(track.cursor.column_index, 0);
        assert_eq!(track.cursor.y, changed_geom.top());
        assert!(!track.cursor.column_nonempty);

        let rtl_layout = Some(Rc::new(SectionColumnLayoutHints::default()));
        track.cursor.y += 18.0;
        track.cursor.column_nonempty = true;
        track.synchronize(PlannedItemContext {
            geom: changed_geom,
            columns: Some(2),
            column_gap_pt: Some(7.0),
            column_layout: &rtl_layout,
            column_rtl: true,
            block_metric: None,
        });

        assert_eq!(track.columns, Some(2));
        assert_eq!(track.column_gap_pt, Some(7.0));
        assert!(track.column_rtl);
        assert_eq!(track.cursor.column_index, 1);
        assert_eq!(track.cursor.y, changed_geom.top());
        assert!(!track.cursor.column_nonempty);
    }

    #[test]
    fn block_projection_state_records_first_page_range_and_width() {
        let mut state = BlockProjectionState::default();
        state.mark_pending(7);
        state.record_pending_page(3);
        state.mark_pending(7);
        state.record_pending_page(4);
        let FlowItem::Line(mut line) = line(10.0) else {
            panic!("line fixture");
        };
        let range = LineCharRange { start: 2, end: 5 };
        line.char_range = Some(range);

        state.record_line(Some(7), &line, 4, 72.0);
        state.record_line(Some(7), &line, 5, f32::NAN);

        assert_eq!(state.block_pages.get(&7), Some(&3));
        assert_eq!(
            state.block_line_pages.get(&7),
            Some(&vec![
                BlockLinePage {
                    page_index: 4,
                    range,
                },
                BlockLinePage {
                    page_index: 5,
                    range,
                },
            ]),
        );
        assert_eq!(state.block_line_widths.get(&7), Some(&vec![72.0]));
    }

    #[test]
    fn streamed_body_metrics_match_the_legacy_scan_for_mixed_entries() {
        let first = page_field_paragraph("1");
        let broken = page_field_paragraph("2");
        let trailing = page_field_paragraph("3");
        let geom = Geom::from_setup(&PageSetup::default());
        let mut capture = LayoutCapture::page_fields();
        let first_indices = reserve_paragraph_page_fields(&first, &mut capture);
        let broken_indices = reserve_paragraph_page_fields(&broken, &mut capture);
        let trailing_indices = reserve_paragraph_page_fields(&trailing, &mut capture);
        let request = |paragraph, page_field_indices| ParagraphFlowRequest {
            paragraph,
            marker: None,
            tab_stops: &[],
            column_break_offsets: &[],
            default_tab_stop_pt: None,
            line_spacing_hint: None,
            geom,
            page_field_indices,
        };
        let mut flow = BodyFlowQueue::default();
        flow.push_ready(FlowItem::PaginationBoundary);
        flow.push_ready(FlowItem::BlockStart {
            index: 0,
            pagination: PaginationHint {
                keep_next: true,
                ..PaginationHint::default()
            },
        });
        flow.push_ready(FlowItem::Gap(2.0));
        flow.push_paragraph(request(&first, first_indices));
        flow.push_ready(FlowItem::Gap(3.0));
        flow.push_ready(FlowItem::BlockStart {
            index: 1,
            pagination: PaginationHint::default(),
        });
        flow.push_ready(FlowItem::Table {
            rows: Vec::new(),
            header_rows: 0,
        });
        flow.push_ready(FlowItem::BlockStart {
            index: 2,
            pagination: PaginationHint::default(),
        });
        flow.push_ready(FlowItem::PageBreak);
        flow.push_paragraph(request(&broken, broken_indices));
        flow.push_ready(FlowItem::PaginationBoundary);
        flow.push_ready(FlowItem::Gap(99.0));
        flow.push_ready(FlowItem::BlockStart {
            index: 3,
            pagination: PaginationHint::default(),
        });
        flow.push_paragraph(request(&trailing, trailing_indices));
        flow.push_ready(FlowItem::Gap(4.0));

        let mut font_cx = strict_font_context(rwml_fonts::noto_sans_kr_subset().to_vec());
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut text_cx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let lowered = lower_body_flow_entries_with_metrics(flow, &mut text_cx, &mut capture);
        let scanned = block_pagination_metrics(&lowered.items);

        assert_eq!(
            metric_snapshots(&lowered.block_metrics),
            metric_snapshots(&scanned)
        );
        assert_eq!(
            dynamic_page_field_indices(&lowered.items),
            vec![Some(0), Some(1), Some(2)]
        );
        assert_eq!(capture.page_fields, vec![None, None, None]);
    }
}
