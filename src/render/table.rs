//! Table grid reconstruction, nested-cell layout, and row fragmentation.

use super::*;

/// A cell placed on the reconstructed grid: its starting column, column span,
/// source cell, and vertical-merge continuity.
struct PlacedCell<'a> {
    col: usize,
    span: usize,
    cell: Option<&'a Cell>,
    continues_from_above: bool,
    continues_below: bool,
}

/// Reconstruct the table grid (re-inserting `row_span` continuation slots so cells
/// land in their true columns) and the total column count.
fn reconstruct_grid(t: &Table) -> (Vec<Vec<PlacedCell<'_>>>, usize) {
    struct Active {
        col: usize,
        span: usize,
        rows_left: usize,
    }
    let mut active: Vec<Active> = Vec::new();
    let mut grid: Vec<Vec<PlacedCell<'_>>> = Vec::with_capacity(t.rows.len());
    let mut ncols = 0usize;
    for (row_index, row) in t.rows.iter().enumerate() {
        let mut placed = Vec::new();
        let mut carried: Vec<Active> = Vec::new();
        let mut col = 0usize;
        let mut ci = 0usize;
        loop {
            if col >= MAX_TABLE_COLS {
                break;
            }
            if let Some(pos) = active.iter().position(|a| a.col == col) {
                let a = active.remove(pos);
                placed.push(PlacedCell {
                    col,
                    span: a.span,
                    cell: None,
                    continues_from_above: true,
                    continues_below: a.rows_left > 1,
                });
                col += a.span;
                if a.rows_left > 1 {
                    carried.push(Active {
                        col: a.col,
                        span: a.span,
                        rows_left: a.rows_left - 1,
                    });
                }
                continue;
            }
            if ci < row.cells.len() {
                let c = &row.cells[ci];
                ci += 1;
                let span = (c.col_span.max(1) as usize).min(MAX_TABLE_COLS);
                let remaining_rows = t.rows.len().saturating_sub(row_index).max(1);
                let rs = (c.row_span.max(1) as usize)
                    .min(MAX_TABLE_COLS)
                    .min(remaining_rows);
                placed.push(PlacedCell {
                    col,
                    span,
                    cell: Some(c),
                    continues_from_above: false,
                    continues_below: rs > 1,
                });
                if rs > 1 {
                    carried.push(Active {
                        col,
                        span,
                        rows_left: rs - 1,
                    });
                }
                col += span;
                continue;
            }
            break;
        }
        ncols = ncols.max(col);
        active.extend(carried);
        active.sort_by_key(|a| a.col);
        grid.push(placed);
    }
    (grid, ncols.max(1))
}

pub(super) fn cell_insets(margins: Option<CellMargins>, width: f32) -> CellInsets {
    let mut insets = margins.map_or(
        CellInsets {
            top: CELL_PAD,
            right: CELL_PAD,
            bottom: CELL_PAD,
            left: CELL_PAD,
        },
        |margins| CellInsets {
            top: (margins.top as f32 / 20.0).min(MAX_CELL_INSET_PT),
            right: (margins.right as f32 / 20.0).min(MAX_CELL_INSET_PT),
            bottom: (margins.bottom as f32 / 20.0).min(MAX_CELL_INSET_PT),
            left: (margins.left as f32 / 20.0).min(MAX_CELL_INSET_PT),
        },
    );
    let available = (width - 1.0).max(0.0);
    let horizontal = insets.left + insets.right;
    if horizontal > available && horizontal > 0.0 {
        let scale = available / horizontal;
        insets.left *= scale;
        insets.right *= scale;
    }
    insets
}

/// The unwrapped (single-line) width of a string at body size — used to size
/// table columns to their content.
fn natural_width(text: &str, cx: &mut TextCx<'_>) -> f32 {
    if text.trim().is_empty() {
        return 0.0;
    }
    let mut b = cx.layout_cx.ranged_builder(cx.font_cx, text, 1.0, false);
    b.push_default(StyleProperty::Brush(rgb::Color::new(0, 0, 0)));
    b.push_default(StyleProperty::FontFamily(font_stack()));
    b.push_default(StyleProperty::FontSize(11.0));
    let mut layout = b.build(text);
    layout.break_all_lines(None);
    layout
        .lines()
        .map(|l| l.metrics().advance)
        .fold(0.0_f32, f32::max)
}

/// Shape a cell's paragraph blocks into wrapped, richly-styled lines (each
/// paragraph keeps its own runs' bold/italic/color/size/font and alignment).
/// Raster images and model-authored charts become atomic row-splitting records.
/// Nested tables reuse the normal grid layout recursively, then expose legal row
/// fragments as cell visuals so outer-row pagination remains bounded. Recursion
/// is depth-capped.
fn cell_visual_line(visual: CellVisual, height: f32, before: f32) -> LineLayout {
    LineLayout {
        height,
        baseline: 0.0,
        clip_to_height: false,
        x_indent: 0.0,
        char_range: None,
        background: None,
        cell_spacing: CellLineSpacing { before, after: 0.0 },
        cell_paragraph: None,
        cell_cant_split_group: None,
        cell_visual: Some(visual),
        leaders: Vec::new(),
        runs: Vec::new(),
    }
}

fn cell_picture_line(
    image: &Image,
    inner_width: f32,
    max_height: f32,
    before: f32,
) -> Option<LineLayout> {
    let available_height = max_height - before;
    let (decoded, width_px, height_px) = decode_model_image(image)?;
    let layout = image_layout(
        width_px,
        height_px,
        image.rotation_degrees,
        inner_width,
        available_height,
    )?;
    Some(cell_visual_line(
        CellVisual::Picture {
            image: decoded,
            layout,
        },
        layout.bounds_h,
        before,
    ))
}

fn cell_chart_line(chart: &Chart, inner_width: f32, max_height: f32) -> Option<LineLayout> {
    let (width, height) = authored_chart_dimensions(chart)?;
    let layout = fit_chart_layout_to_box(width, height, inner_width, max_height)?;
    Some(cell_visual_line(
        CellVisual::Chart {
            chart: chart.clone(),
            width,
            height,
            layout,
        },
        layout.bounds_h,
        0.0,
    ))
}

fn nested_table_geom(inner_width: f32, max_height: f32) -> Geom {
    Geom {
        page_w: inner_width.max(20.0),
        page_h: max_height.max(1.0),
        left: 0.0,
        right: 0.0,
        top_m: 0.0,
        bottom_m: 0.0,
    }
}

fn nested_row_visual_lines(
    rows: Vec<RowLayout>,
    max_height: f32,
    state: &mut CellShapeState,
) -> Vec<LineLayout> {
    let mut lines = Vec::new();
    for mut row in rows {
        if lines.len() >= MAX_CELL_LINES {
            break;
        }
        let keep_whole = row.cant_split && row.height <= max_height;
        let group = row
            .cant_split
            .then(|| state.allocate_cant_split_group())
            .flatten();
        loop {
            let legal_budget = if keep_whole {
                row.height
            } else {
                first_row_fragment_height(&row)
            };
            let budget = legal_budget.min(max_height.max(1.0));
            let (fragment, rest) = if row.height <= budget + f32::EPSILON {
                (row, None)
            } else {
                split_row(row, budget)
            };
            let height = fragment.height;
            let mut line = cell_visual_line(
                CellVisual::NestedRow {
                    row: Box::new(fragment),
                },
                height,
                0.0,
            );
            line.cell_cant_split_group = group;
            lines.push(line);
            if lines.len() >= MAX_CELL_LINES {
                break;
            }
            let Some(remaining) = rest else {
                break;
            };
            row = remaining;
        }
    }
    lines
}

#[allow(clippy::too_many_arguments)]
fn shape_nested_table(
    table: &Table,
    hints: Option<&TablePaginationHints>,
    default_tab_stop_pt: Option<f32>,
    inner_width: f32,
    max_height: f32,
    depth: u32,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    state: &mut CellShapeState,
    lists: &mut ListState,
) -> Vec<LineLayout> {
    if depth > MAX_CELL_DEPTH {
        return Vec::new();
    }
    let mut flow = Vec::new();
    layout_table_with_row_pagination_and_lists(
        table,
        &mut flow,
        nested_table_geom(inner_width, max_height),
        cx,
        capture,
        TablePaginationView {
            rows: hints.map(|hints| hints.rows.as_slice()),
            cells: hints.map(|hints| &hints.cells),
            cell_line_spacing: hints.map(|hints| &hints.cell_line_spacing),
            nested: hints.map(|hints| &hints.nested),
            cell_tabs: hints.map(|hints| &hints.cell_tabs),
            default_tab_stop_pt,
            depth,
        },
        lists,
    );
    let Some(FlowItem::Table { rows, .. }) = flow.pop() else {
        return Vec::new();
    };
    nested_row_visual_lines(rows, max_height, state)
}

#[cfg(test)]
pub(super) fn shape_cell(
    cell: &Cell,
    inner_w: f32,
    depth: u32,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) -> Vec<LineLayout> {
    shape_cell_with_pagination(
        cell, None, None, None, None, None, inner_w, depth, cx, capture,
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn shape_cell_with_pagination(
    cell: &Cell,
    pagination: Option<&[Option<PaginationHint>]>,
    line_spacing: Option<&[Option<LineSpacingHint>]>,
    tab_stops: Option<&[Vec<TabStop>]>,
    nested_pagination: Option<&[Option<TablePaginationHints>]>,
    default_tab_stop_pt: Option<f32>,
    inner_w: f32,
    depth: u32,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) -> Vec<LineLayout> {
    let mut lists = ListState::default();
    shape_cell_with_pagination_and_lists(
        cell,
        pagination,
        line_spacing,
        tab_stops,
        nested_pagination,
        default_tab_stop_pt,
        inner_w,
        (PAGE_H - 2.0 * MARGIN).max(1.0),
        depth,
        cx,
        capture,
        &mut lists,
    )
}

#[allow(clippy::too_many_arguments)]
fn shape_cell_with_pagination_and_lists(
    cell: &Cell,
    pagination: Option<&[Option<PaginationHint>]>,
    line_spacing: Option<&[Option<LineSpacingHint>]>,
    tab_stops: Option<&[Vec<TabStop>]>,
    nested_pagination: Option<&[Option<TablePaginationHints>]>,
    default_tab_stop_pt: Option<f32>,
    inner_w: f32,
    max_visual_height: f32,
    depth: u32,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    lists: &mut ListState,
) -> Vec<LineLayout> {
    let mut state = CellShapeState {
        next_paragraph_id: 0,
        next_cant_split_group_id: 1,
    };
    shape_cell_in_scope(
        cell,
        pagination,
        line_spacing,
        tab_stops,
        nested_pagination,
        default_tab_stop_pt,
        inner_w,
        max_visual_height,
        depth,
        cx,
        capture,
        0,
        &mut state,
        lists,
    )
}

struct CellShapeState {
    next_paragraph_id: usize,
    next_cant_split_group_id: usize,
}

fn explicit_cell_spacing(value: Option<f32>) -> f32 {
    value
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(0.0)
}

pub(super) fn truncate_cell_paragraph_lines(
    lines: &mut Vec<LineLayout>,
    remaining: usize,
    spacing: Spacing,
) {
    let retained_all = lines.len() <= remaining;
    lines.truncate(remaining);
    if let Some(first) = lines.first_mut() {
        first.cell_spacing.before = explicit_cell_spacing(spacing.before_pt);
    }
    if retained_all {
        if let Some(last) = lines.last_mut() {
            last.cell_spacing.after = explicit_cell_spacing(spacing.after_pt);
        }
    }
}

impl CellShapeState {
    fn allocate_paragraph(&mut self) -> usize {
        let value = self.next_paragraph_id;
        self.next_paragraph_id = self.next_paragraph_id.saturating_add(1);
        value
    }

    fn allocate_cant_split_group(&mut self) -> Option<NonZeroUsize> {
        let value = NonZeroUsize::new(self.next_cant_split_group_id);
        self.next_cant_split_group_id = self.next_cant_split_group_id.checked_add(1).unwrap_or(0);
        value
    }
}

#[allow(clippy::too_many_arguments)]
fn shape_cell_in_scope(
    cell: &Cell,
    pagination: Option<&[Option<PaginationHint>]>,
    line_spacing: Option<&[Option<LineSpacingHint>]>,
    tab_stops: Option<&[Vec<TabStop>]>,
    nested_pagination: Option<&[Option<TablePaginationHints>]>,
    default_tab_stop_pt: Option<f32>,
    inner_w: f32,
    max_visual_height: f32,
    depth: u32,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    scope_id: usize,
    state: &mut CellShapeState,
    lists: &mut ListState,
) -> Vec<LineLayout> {
    let mut lines = Vec::new();
    if depth > MAX_CELL_DEPTH {
        return lines;
    }
    for (block_index, b) in cell.blocks.iter().enumerate() {
        // Bound a pathologically tall cell so the page-split paginator stays linear.
        if lines.len() >= MAX_CELL_LINES {
            break;
        }
        match b {
            Block::Paragraph(p) => {
                let marker = paragraph_list_marker(p, lists);
                let paragraph_tab_stops = tab_stops
                    .and_then(|stops| stops.get(block_index))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let line_spacing_hint = line_spacing
                    .and_then(|hints| hints.get(block_index))
                    .copied()
                    .flatten();
                let ShapedParagraph {
                    lines: mut paragraph_lines,
                    images: paragraph_images,
                } = shape_paragraph_content(
                    p,
                    marker.as_deref(),
                    paragraph_tab_stops,
                    default_tab_stop_pt,
                    line_spacing_hint,
                    inner_w,
                    cx,
                    capture,
                    false,
                );
                if paragraph_lines.is_empty() && paragraph_images.is_empty() {
                    continue;
                }
                let remaining = MAX_CELL_LINES.saturating_sub(lines.len());
                truncate_cell_paragraph_lines(&mut paragraph_lines, remaining, p.props.spacing);
                if !paragraph_lines.is_empty() {
                    if let Some(hint) = pagination
                        .and_then(|hints| hints.get(block_index))
                        .copied()
                        .flatten()
                    {
                        let paragraph_id = state.allocate_paragraph();
                        let line_count = paragraph_lines.len();
                        for (line_index, line) in paragraph_lines.iter_mut().enumerate() {
                            line.cell_paragraph = Some(CellParagraphLine {
                                scope_id,
                                paragraph_id,
                                line_index,
                                line_count,
                                pagination: hint,
                            });
                        }
                    }
                }
                lines.extend(paragraph_lines);
                for image in paragraph_images {
                    if lines.len() >= MAX_CELL_LINES {
                        break;
                    }
                    if let Some(line) =
                        cell_picture_line(image, inner_w, max_visual_height, PARA_GAP)
                    {
                        lines.push(line);
                    }
                }
            }
            Block::Table(t) => {
                let table_pagination = nested_pagination
                    .and_then(|tables| tables.get(block_index))
                    .and_then(Option::as_ref);
                lines.extend(shape_nested_table(
                    t,
                    table_pagination,
                    default_tab_stop_pt,
                    inner_w,
                    max_visual_height,
                    depth + 1,
                    cx,
                    capture,
                    state,
                    lists,
                ));
            }
            Block::Image(image) => {
                if let Some(line) = cell_picture_line(image, inner_w, max_visual_height, 0.0) {
                    lines.push(line);
                }
            }
            Block::Chart(chart) => {
                if let Some(line) = cell_chart_line(chart, inner_w, max_visual_height) {
                    lines.push(line);
                }
            }
            Block::PageBreak | Block::SectionBreak(_) => {}
        }
    }
    lines.truncate(MAX_CELL_LINES);
    lines
}

/// Lay out a table into one [`FlowItem::Row`] per row. Column widths come from the
/// model's authored `col_widths_pct` when present; otherwise columns are sized to
/// their content (natural widths scaled to fill the content box), so a narrow
/// label column and a wide value column read correctly instead of being equal.
#[cfg(test)]
pub(super) fn layout_table(
    t: &Table,
    out: &mut Vec<FlowItem>,
    geom: Geom,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) {
    layout_table_with_row_pagination(t, out, geom, cx, capture, TablePaginationView::default());
}

#[derive(Clone, Copy, Default)]
pub(super) struct TablePaginationView<'a> {
    pub(super) rows: Option<&'a [TableRowPaginationHint]>,
    pub(super) cells: Option<&'a TableCellPaginationHints>,
    pub(super) cell_line_spacing: Option<&'a TableCellLineSpacingHints>,
    pub(super) nested: Option<&'a TableCellNestedPaginationHints>,
    pub(super) cell_tabs: Option<&'a TableCellTabStopHints>,
    pub(super) default_tab_stop_pt: Option<f32>,
    pub(super) depth: u32,
}

fn table_placement(t: &Table, available_width: f32) -> (f32, f32) {
    let available_width = if available_width.is_finite() && available_width > 0.0 {
        available_width
    } else {
        1.0
    };
    let width = t
        .width_pct
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| available_width * value.min(1.0))
        .unwrap_or(available_width)
        .clamp(1.0, available_width);
    let slack = (available_width - width).max(0.0);
    // ECMA-376 17.4.29/17.4.51/17.4.64: table alignment is logical
    // under bidiVisual, and leading indentation applies only at that edge.
    let logical_x = match t.align.unwrap_or(Align::Left) {
        Align::Left => t.indent_twips.unwrap_or(0).max(0) as f32 / 20.0,
        Align::Center => slack * 0.5,
        Align::Right => slack,
        Align::Justify => 0.0,
    }
    .clamp(0.0, slack);
    let x = if t.bidi_visual {
        slack - logical_x
    } else {
        logical_x
    };
    (x, width)
}

fn table_border_line_paint(t: &Table, side: TableBorderSide) -> TableBorderPaint {
    let color = t
        .border_colors
        .get(side)
        .or(t.border_color)
        .map(|color| rgb::Color::new(color.r, color.g, color.b))
        .unwrap_or_else(rgb::Color::black);
    // ECMA-376 Part 1 CT_Border.sz / 17.18.23: line widths are
    // eighth-points and values above 96 may be reassigned. The public model
    // permits 1, so preserve every positive model value below that ceiling.
    let size = t
        .border_sizes
        .get(side)
        .filter(|size| *size > 0)
        .or(t.border_size_eighths.filter(|size| *size > 0));
    let width = match size {
        Some(size) if size > 0 => f32::from(size.min(MAX_TABLE_BORDER_SIZE_EIGHTHS)) / 8.0,
        _ => BORDER,
    };
    TableBorderPaint { color, width }
}

pub(super) fn table_border_paints(t: &Table) -> TableBorderPaints {
    TableBorderPaints {
        top: table_border_line_paint(t, TableBorderSide::Top),
        left: table_border_line_paint(t, TableBorderSide::Left),
        bottom: table_border_line_paint(t, TableBorderSide::Bottom),
        right: table_border_line_paint(t, TableBorderSide::Right),
        inside_h: table_border_line_paint(t, TableBorderSide::InsideHorizontal),
        inside_v: table_border_line_paint(t, TableBorderSide::InsideVertical),
    }
}

fn bound_table_border_paints_to_rows(
    paints: TableBorderPaints,
    rows: &[RowLayout],
) -> TableBorderPaints {
    let max_width = rows
        .iter()
        .flat_map(|row| {
            row.cells.iter().filter_map(move |cell| {
                (row.height.is_finite()
                    && row.height > 0.0
                    && cell.width.is_finite()
                    && cell.width > 0.0)
                    .then_some(cell.width.min(row.height) * 0.5)
            })
        })
        .fold(f32::INFINITY, f32::min);
    paints.with_max_width(max_width)
}

fn authored_table_column_edges(widths: &[f32], ncols: usize, content_w: f32) -> Option<Vec<f32>> {
    if widths.len() != ncols
        || widths
            .iter()
            .any(|width| !width.is_finite() || *width <= 0.0)
    {
        return None;
    }
    let sum = widths.iter().map(|width| f64::from(*width)).sum::<f64>();
    if !sum.is_finite() || sum <= 0.0 {
        return None;
    }

    let mut edges = Vec::with_capacity(ncols + 1);
    edges.push(0.0);
    let mut cumulative = 0.0_f64;
    for width in widths {
        cumulative += f64::from(*width);
        let edge = ((f64::from(content_w) * cumulative / sum) as f32).min(content_w);
        if !edge.is_finite() || edge <= *edges.last()? {
            return None;
        }
        edges.push(edge);
    }
    *edges.last_mut()? = content_w;
    Some(edges)
}

#[cfg(test)]
pub(super) fn layout_table_with_row_pagination(
    t: &Table,
    out: &mut Vec<FlowItem>,
    geom: Geom,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    pagination: TablePaginationView<'_>,
) {
    let mut lists = ListState::default();
    layout_table_with_row_pagination_and_lists(t, out, geom, cx, capture, pagination, &mut lists);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn layout_table_with_row_pagination_and_lists(
    t: &Table,
    out: &mut Vec<FlowItem>,
    geom: Geom,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    pagination: TablePaginationView<'_>,
    lists: &mut ListState,
) {
    let table_id = capture.allocate_table_id();
    let (grid, ncols) = reconstruct_grid(t);
    let (table_x, content_w) = table_placement(t, geom.content_w());
    let border = table_border_paints(t);

    // Column edges: honor authored percentages when they match the grid, else
    // size to content (min 20pt/col) and scale to fill the content width.
    let col_x =
        if let Some(edges) = authored_table_column_edges(&t.col_widths_pct, ncols, content_w) {
            edges
        } else {
            let mut edges = vec![0.0_f32; ncols + 1];
            let mut col_nat = vec![20.0_f32; ncols];
            for placed_row in &grid {
                for pc in placed_row {
                    if let Some(c) = pc.cell {
                        let txt = c.text().replace('\n', " ");
                        let insets = cell_insets(c.margins, content_w);
                        let per = (natural_width(&txt, cx) + insets.left + insets.right)
                            / pc.span.max(1) as f32;
                        for slot in col_nat
                            .iter_mut()
                            .take((pc.col + pc.span).min(ncols))
                            .skip(pc.col)
                        {
                            *slot = slot.max(per);
                        }
                    }
                }
            }
            let total: f32 = col_nat.iter().sum();
            let scale = if total > 0.0 { content_w / total } else { 1.0 };
            for c in 0..ncols {
                edges[c + 1] = edges[c] + col_nat[c] * scale;
            }
            edges
        };

    // Pass 2: shape each cell richly at its column width and build the rows.
    let mut rows: Vec<RowLayout> = Vec::with_capacity(grid.len());
    for (row_index, placed_row) in grid.into_iter().enumerate() {
        let mut cells = Vec::with_capacity(placed_row.len());
        let mut row_h = 0.0_f32;
        let row_cell_pagination = pagination.cells.and_then(|rows| rows.get(row_index));
        let row_cell_line_spacing = pagination
            .cell_line_spacing
            .and_then(|rows| rows.get(row_index));
        let row_nested_pagination = pagination.nested.and_then(|rows| rows.get(row_index));
        let row_cell_tab_stops = pagination.cell_tabs.and_then(|rows| rows.get(row_index));
        let mut source_cell_index = 0usize;
        for pc in placed_row {
            let end = (pc.col + pc.span).min(ncols);
            let logical_x = col_x[pc.col];
            let width = col_x[end] - logical_x;
            let (visual_left, visual_right) = if t.bidi_visual {
                (content_w - col_x[end], content_w - logical_x)
            } else {
                (logical_x, col_x[end])
            };
            let x = table_x + visual_left;
            let right = table_x + visual_right;
            let left_outer = if t.bidi_visual {
                end == ncols
            } else {
                pc.col == 0
            };
            let right_outer = if t.bidi_visual {
                pc.col == 0
            } else {
                end == ncols
            };
            let border_edges = CellBorderEdges {
                top: if pc.continues_from_above {
                    None
                } else if row_index == 0 {
                    Some(TableBorderSide::Top)
                } else {
                    Some(TableBorderSide::InsideHorizontal)
                },
                left: Some(if left_outer {
                    TableBorderSide::Left
                } else {
                    TableBorderSide::InsideVertical
                }),
                bottom: if pc.continues_below {
                    None
                } else if row_index + 1 == t.rows.len() {
                    Some(TableBorderSide::Bottom)
                } else {
                    Some(TableBorderSide::InsideHorizontal)
                },
                right: Some(if right_outer {
                    TableBorderSide::Right
                } else {
                    TableBorderSide::InsideVertical
                }),
            };
            let (
                direct_pagination,
                direct_line_spacing,
                direct_tab_stops,
                direct_nested_pagination,
            ) = if pc.cell.is_some() {
                let paragraph_hints = row_cell_pagination
                    .and_then(|cells| cells.get(source_cell_index))
                    .map(Vec::as_slice);
                let paragraph_line_spacing = row_cell_line_spacing
                    .and_then(|cells| cells.get(source_cell_index))
                    .map(Vec::as_slice);
                let paragraph_tab_stops = row_cell_tab_stops
                    .and_then(|cells| cells.get(source_cell_index))
                    .map(Vec::as_slice);
                let nested_hints = row_nested_pagination
                    .and_then(|cells| cells.get(source_cell_index))
                    .map(Vec::as_slice);
                source_cell_index += 1;
                (
                    paragraph_hints,
                    paragraph_line_spacing,
                    paragraph_tab_stops,
                    nested_hints,
                )
            } else {
                (None, None, None, None)
            };
            let (lines, insets, shading, valign) = match pc.cell {
                Some(c) => {
                    let insets = cell_insets(c.margins, width);
                    let lines = shape_cell_with_pagination_and_lists(
                        c,
                        direct_pagination,
                        direct_line_spacing,
                        direct_tab_stops,
                        direct_nested_pagination,
                        pagination.default_tab_stop_pt,
                        (width - insets.left - insets.right).max(1.0),
                        (geom.bottom() - geom.top() - insets.top - insets.bottom).max(1.0),
                        pagination.depth,
                        cx,
                        capture,
                        lists,
                    );
                    let shading = c.shading.map(|s| rgb::Color::new(s.r, s.g, s.b));
                    (lines, insets, shading, c.valign)
                }
                None => (Vec::new(), cell_insets(None, width), None, VCell::Top),
            };
            let content_h = cell_lines_extent(&lines);
            row_h = row_h.max(content_h + insets.top + insets.bottom);
            cells.push(CellBox {
                x,
                right,
                width,
                lines,
                insets,
                shading,
                valign,
                border_edges,
            });
        }
        // A minimum row height so empty rows still draw a band.
        row_h = row_h.max(14.0);
        rows.push(RowLayout {
            height: row_h,
            cells,
            cant_split: pagination
                .rows
                .and_then(|rows| rows.get(row_index))
                .map(|row| row.cant_split)
                .unwrap_or(true),
            border,
            table_id: Some(table_id),
        });
    }
    let border = bound_table_border_paints_to_rows(border, &rows);
    for row in &mut rows {
        row.border = border;
    }
    let header_rows = t.header_rows.min(rows.len());
    out.push(FlowItem::Table { rows, header_rows });
}

/// Split a row into a fragment that fits `avail` points of height and the leftover
/// rest, by partitioning each cell's lines. At least one line is always kept in
/// the fragment so progress is guaranteed even for a line taller than a page.
pub(super) fn legal_cell_split(lines: &[LineLayout], cut: usize) -> bool {
    if cut == 0 {
        return false;
    }
    if cut >= lines.len() {
        return true;
    }
    if let (Some(before), Some(after)) = (
        lines[cut - 1].cell_cant_split_group,
        lines[cut].cell_cant_split_group,
    ) {
        if before == after {
            return false;
        }
    }
    let (Some(before), Some(after)) = (lines[cut - 1].cell_paragraph, lines[cut].cell_paragraph)
    else {
        return true;
    };
    if before.scope_id != after.scope_id {
        return true;
    }
    if before.paragraph_id != after.paragraph_id {
        return !before.pagination.keep_next;
    }
    if before.pagination.keep_lines || before.pagination.keep_next {
        return false;
    }
    if !before.pagination.widow_control {
        return true;
    }
    let leading = before.line_index.saturating_add(1);
    let trailing = before.line_count.saturating_sub(leading);
    before.line_count > 3 && leading >= 2 && trailing >= 2
}

fn greedy_cell_split(lines: &[LineLayout], budget: f32) -> usize {
    let mut used = 0.0_f32;
    let mut count = 0usize;
    for line in lines {
        let extent = line.cell_extent();
        if count == 0 || used + extent <= budget {
            used += extent;
            count += 1;
        } else {
            break;
        }
    }
    count
}

fn fitting_nonterminal_cell_split(lines: &[LineLayout], budget: f32) -> usize {
    if lines.len() <= 1 {
        return lines.len();
    }
    let greedy = greedy_cell_split(lines, budget).min(lines.len() - 1);
    (1..=greedy)
        .rev()
        .find(|cut| legal_cell_split(lines, *cut))
        .unwrap_or(greedy)
}

fn fit_forced_cell_visual_to_budget(lines: &mut [LineLayout], budget: f32) {
    if lines.len() != 1 || !budget.is_finite() || budget <= 0.0 || lines[0].cell_extent() <= budget
    {
        return;
    }
    let line = &mut lines[0];
    let Some(visual) = line.cell_visual.as_mut() else {
        return;
    };
    let before = if line.cell_spacing.before < budget {
        line.cell_spacing.before
    } else {
        0.0
    };
    let Some(height) = visual.fit_to_height(budget - before) else {
        return;
    };
    line.height = height;
    line.cell_spacing.before = before;
}

pub(super) fn split_row(row: RowLayout, avail: f32) -> (RowLayout, Option<RowLayout>) {
    let cant_split = row.cant_split;
    let border = row.border;
    let table_id = row.table_id;
    let mut frag_cells = Vec::with_capacity(row.cells.len());
    let mut rest_cells = Vec::with_capacity(row.cells.len());
    let mut any_rest = false;
    for cell in row.cells {
        let CellBox {
            x,
            right,
            width,
            shading,
            valign,
            lines,
            insets,
            border_edges,
        } = cell;
        let content_budget = (avail - insets.top).max(0.0);
        let cut = if cell_lines_extent(&lines) + insets.bottom <= content_budget {
            lines.len()
        } else {
            // A nonterminal fragment drops its bottom inset, so padding must not
            // reduce its legal line budget. Preserve at least one line for the
            // terminal fragment when only that inset exceeds the page budget.
            fitting_nonterminal_cell_split(&lines, content_budget)
        };
        let mut head = lines;
        let tail = head.split_off(cut);
        let forced_visual_budget = if tail.is_empty() {
            (content_budget - insets.bottom).max(0.0)
        } else {
            content_budget
        };
        fit_forced_cell_visual_to_budget(&mut head, forced_visual_budget);
        if !tail.is_empty() {
            any_rest = true;
        }
        let has_tail = !tail.is_empty();
        frag_cells.push(CellBox {
            x,
            right,
            width,
            shading,
            valign,
            insets: if has_tail {
                CellInsets {
                    bottom: 0.0,
                    ..insets
                }
            } else {
                insets
            },
            lines: head,
            border_edges,
        });
        rest_cells.push(CellBox {
            x,
            right,
            width,
            shading,
            valign,
            insets: if has_tail {
                CellInsets { top: 0.0, ..insets }
            } else {
                CellInsets::zero()
            },
            lines: tail,
            border_edges,
        });
    }
    if any_rest {
        for cell in &mut frag_cells {
            cell.border_edges.bottom = None;
        }
        for cell in &mut rest_cells {
            cell.border_edges.top = None;
        }
    }
    let frag = RowLayout {
        height: avail,
        cells: frag_cells,
        cant_split,
        border,
        table_id,
    };
    if any_rest {
        let rest_h = rest_cells
            .iter()
            .map(|c| cell_lines_extent(&c.lines) + c.insets.top + c.insets.bottom)
            .fold(0.0_f32, f32::max);
        let rest = RowLayout {
            height: rest_h.max(14.0),
            cells: rest_cells,
            cant_split,
            border,
            table_id,
        };
        (frag, Some(rest))
    } else {
        (frag, None)
    }
}
