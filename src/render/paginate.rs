//! Bounded flow placement across section pages and columns.

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

fn record_pending_block_page(
    block_pages: &mut HashMap<usize, usize>,
    pending_block: &mut Option<usize>,
    page_index: usize,
) {
    if let Some(block_index) = pending_block.take() {
        block_pages.entry(block_index).or_insert(page_index);
    }
}

fn record_block_line_page(
    block_line_pages: &mut HashMap<usize, Vec<BlockLinePage>>,
    current_block: Option<usize>,
    line: &LineLayout,
    page_index: usize,
) {
    let (Some(block_index), Some(range)) = (current_block, line.char_range) else {
        return;
    };
    block_line_pages
        .entry(block_index)
        .or_default()
        .push(BlockLinePage { page_index, range });
}

fn record_block_line_width(
    block_line_widths: &mut HashMap<usize, Vec<f32>>,
    current_block: Option<usize>,
    width: f32,
) {
    let Some(block_index) = current_block else {
        return;
    };
    if width.is_finite() && width > 0.0 {
        block_line_widths
            .entry(block_index)
            .or_default()
            .push(width);
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

#[derive(Clone)]
struct BlockPaginationMetrics {
    pagination: PaginationHint,
    next_start: Option<usize>,
    line_heights: Vec<f32>,
    first_line_extent: f32,
    last_line_extent: f32,
    total_height: f32,
    is_paragraph: bool,
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
        let mut line_heights = Vec::new();
        let mut extent = 0.0;
        let mut first_line_extent = None;
        let mut last_line_extent = 0.0;
        let mut is_paragraph = true;
        for item in &items[start + 1..end] {
            match item {
                FlowItem::Gap(height) => extent += height.max(0.0),
                FlowItem::Line(line) => {
                    let height = line.height.max(0.0);
                    extent += height;
                    first_line_extent.get_or_insert(extent);
                    last_line_extent = extent;
                    line_heights.push(height);
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
                | FlowItem::Chart { .. } => is_paragraph = false,
            }
        }
        is_paragraph &= !line_heights.is_empty();
        metrics[start] = Some(BlockPaginationMetrics {
            pagination,
            next_start,
            line_heights,
            first_line_extent: first_line_extent.unwrap_or(0.0),
            last_line_extent,
            total_height: extent,
            is_paragraph,
        });
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

pub(super) fn paginate_with_column_gap(
    items: Vec<FlowItem>,
    geom: Geom,
    final_section_setup: &SectionSetup,
    final_column_gap_pt: Option<f32>,
    final_column_layout: Option<&SectionColumnLayoutHints>,
    final_column_rtl: bool,
) -> Pagination {
    // Paginate flow items top-to-bottom through section columns and then across
    // pages. Tables repeat headers after each break and split oversized rows.
    let columns_by_item = section_columns_by_item(&items, final_section_setup.columns);
    let column_gaps_by_item = section_column_gaps_by_item(&items, final_column_gap_pt);
    let column_layouts_by_item = section_column_layouts_by_item(&items, final_column_layout);
    let column_rtl_by_item = section_column_rtl_by_item(&items, final_column_rtl);
    let geometries_by_item = section_geometries_by_item(&items, geom);
    let block_metrics = block_pagination_metrics(&items);
    let mut pages: Pages = vec![Vec::new()];
    let mut page_sections: Vec<Option<RenderPageSection>> = vec![None];
    let mut section_start_page_index = 0usize;
    let mut section_index = 0usize;
    let mut active_geom = geometries_by_item.first().copied().unwrap_or(geom);
    let mut active_columns = columns_by_item
        .first()
        .copied()
        .unwrap_or(final_section_setup.columns);
    let mut active_column_gap_pt = column_gaps_by_item
        .first()
        .copied()
        .unwrap_or(final_column_gap_pt);
    let mut active_column_layout = column_layouts_by_item.first().cloned().flatten();
    let mut cursor = FlowCursor::new(
        active_geom,
        active_columns,
        active_column_gap_pt,
        active_column_layout.as_deref(),
        column_rtl_by_item
            .first()
            .copied()
            .unwrap_or(final_column_rtl),
    );
    let mut active_column_rtl = column_rtl_by_item
        .first()
        .copied()
        .unwrap_or(final_column_rtl);
    let mut block_pages = HashMap::new();
    let mut block_line_pages: HashMap<usize, Vec<BlockLinePage>> = HashMap::new();
    let mut block_line_widths: HashMap<usize, Vec<f32>> = HashMap::new();
    let mut pending_block = None;
    let mut current_block = None;
    let mut current_block_start = None;
    let mut current_line_index = 0usize;
    let mut widow_break_before = None;
    let mut pending_top_bottom_bands = Vec::new();
    let mut active_top_bottom_bands = Vec::new();
    let mut deferred_top_bottom_bands = Vec::new();
    let mut previous_keep_next = false;
    let mut defer_current_top_bottom_bands = false;
    for (item_index, item) in items.into_iter().enumerate() {
        let item_geom = geometries_by_item[item_index];
        if item_geom != active_geom {
            active_geom = item_geom;
            cursor.set_columns(
                active_geom,
                columns_by_item[item_index],
                column_gaps_by_item[item_index],
                column_layouts_by_item[item_index].as_deref(),
                column_rtl_by_item[item_index],
            );
            active_columns = columns_by_item[item_index];
            active_column_gap_pt = column_gaps_by_item[item_index];
            active_column_layout = column_layouts_by_item[item_index].clone();
            active_column_rtl = column_rtl_by_item[item_index];
        }
        let item_columns = columns_by_item[item_index];
        let item_column_gap_pt = column_gaps_by_item[item_index];
        let item_column_layout = &column_layouts_by_item[item_index];
        if item_columns != active_columns
            || item_column_gap_pt != active_column_gap_pt
            || !same_section_column_layout(item_column_layout, &active_column_layout)
            || column_rtl_by_item[item_index] != active_column_rtl
        {
            cursor.set_columns(
                active_geom,
                item_columns,
                item_column_gap_pt,
                item_column_layout.as_deref(),
                column_rtl_by_item[item_index],
            );
            active_columns = item_columns;
            active_column_gap_pt = item_column_gap_pt;
            active_column_layout = item_column_layout.clone();
            active_column_rtl = column_rtl_by_item[item_index];
        }
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
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                if let Some(metric) = block_metrics[item_index].as_ref() {
                    if pagination.keep_next {
                        if let Some(height) =
                            keep_next_chain_height(item_index, &block_metrics, &columns_by_item)
                        {
                            move_to_fresh_column_for_required_height(
                                &mut pages,
                                &mut cursor,
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
                            &mut cursor,
                            metric.last_line_extent,
                            active_geom,
                            &active_top_bottom_bands,
                        );
                    }
                }
                pending_block = Some(block_index);
                current_block = Some(block_index);
                current_block_start = Some(item_index);
                current_line_index = 0;
                widow_break_before = None;
            }
            FlowItem::PaginationBoundary => {
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
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
                    &mut cursor,
                    h,
                    active_geom,
                    &active_top_bottom_bands,
                    None,
                );
                if let Some(metric) = current_block_start
                    .and_then(|start| block_metrics.get(start))
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
                    &mut cursor,
                    h,
                    active_geom,
                    &active_top_bottom_bands,
                    None,
                );
                let page_index = pages.len().saturating_sub(1);
                record_pending_block_page(&mut block_pages, &mut pending_block, page_index);
                record_block_line_page(&mut block_line_pages, current_block, &l, page_index);
                record_block_line_width(
                    &mut block_line_widths,
                    current_block,
                    cursor.columns.width(cursor.column_index),
                );
                let line_range = l.char_range;
                place_item(&mut pages, &mut cursor, FlowItem::Line(l), h);
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
                    &mut cursor,
                    layout.bounds_h,
                    active_geom,
                    &active_top_bottom_bands,
                    current_block,
                );
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                place_item(
                    &mut pages,
                    &mut cursor,
                    FlowItem::Picture { image, layout },
                    layout.bounds_h,
                );
            }
            FlowItem::Chart { chart, w, h } => {
                ensure_outside_top_bottom_bands(
                    &mut pages,
                    &mut cursor,
                    h,
                    active_geom,
                    &active_top_bottom_bands,
                    None,
                );
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                place_item(&mut pages, &mut cursor, FlowItem::Chart { chart, w, h }, h);
            }
            FlowItem::Table { rows, header_rows } => {
                let fallback_page = pages.len().saturating_sub(1);
                let first_page =
                    place_table(&mut pages, &mut cursor, rows, header_rows, active_geom)
                        .unwrap_or(fallback_page);
                record_pending_block_page(&mut block_pages, &mut pending_block, first_page);
            }
            FlowItem::PageBreak => {
                cursor.force_page(&mut pages, active_geom);
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
            }
            FlowItem::ColumnBreak => {
                cursor.advance(&mut pages, active_geom);
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
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
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                section_start_page_index = next_section_page.saturating_sub(1);
                section_index = section_index.saturating_add(1);
            }
            // Rows reach pagination only inside a Table; place defensively.
            FlowItem::Row(r) => {
                let h = r.height;
                ensure_outside_top_bottom_bands(
                    &mut pages,
                    &mut cursor,
                    h,
                    active_geom,
                    &active_top_bottom_bands,
                    None,
                );
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                place_item(&mut pages, &mut cursor, FlowItem::Row(r), h);
            }
        }
    }
    record_pending_block_page(
        &mut block_pages,
        &mut pending_block,
        pages.len().saturating_sub(1),
    );
    page_sections.resize(pages.len(), None);
    assign_section_to_render_pages(
        &mut page_sections,
        section_start_page_index,
        pages.len().saturating_sub(1),
        final_section_setup,
        section_index,
    );
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
    paginate_with_column_gap(
        flow.lower(cx, capture),
        geom,
        final_section_setup,
        final_column_gap_pt,
        final_column_layout,
        final_column_rtl,
    )
}
