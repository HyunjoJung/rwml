//! Native typesetting renderer: `DocModel` → A4 **PDF** via `parley` (layout,
//! shaping, Korean/CJK line-breaking + font fallback) and `krilla` (PDF emit with
//! subsetted embedded fonts and selectable text). Behind the `render` feature.
//!
//! Pipeline: blocks are laid out into a stream of flow items — text lines and
//! table rows — which are flowed top-to-bottom onto fixed A4 pages, then each
//! page's glyph runs and table borders are drawn with krilla. A table that spans
//! pages repeats its header rows after each break, and a row taller than a page is
//! split across pages at line boundaries. Tables are rendered as a real grid:
//! columns are reconstructed
//! (including `col_span`/`row_span` placement), sized to authored `col_widths_pct`
//! or to content, then bordered; cells carry rich per-run text (bold/italic/color/
//! size/font), background shading, and vertical alignment. Images (block-level
//! and inline) are decoded and drawn as raster pictures, fit to the content box.
//!
//! Fonts come from the system font collection (parley's default `FontContext`),
//! so Korean renders when a Hangul-capable face is installed. A bundled font is a
//! later, license-gated addition.

use std::borrow::Cow;
use std::collections::HashMap;
use std::rc::Rc;

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Size, Transform};
use krilla::image::Image as PdfImage;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla::{Data, Document as PdfDoc};
use parley::layout::Alignment;
use parley::style::{FontFamily, FontFamilyName, FontStyle, FontWeight, StyleProperty};
use parley::{FontContext, LayoutContext};

use crate::model::{
    Align, Block, Cell, CharProps, DocModel, Image, ListInfo, Paragraph, Table, VCell,
};

// A4 page geometry, in PDF points.
const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 56.0;
const CONTENT_W: f32 = PAGE_W - 2.0 * MARGIN;
const TOP: f32 = MARGIN;
const BOTTOM: f32 = PAGE_H - MARGIN;
const PARA_GAP: f32 = 6.0;
const CELL_PAD: f32 = 3.0;
const BORDER: f32 = 0.4;
/// Left indent added per list nesting level, in points.
const LIST_INDENT: f32 = 18.0;
/// Hard cap on grid columns / a cell's column or row span, so a hostile model
/// (e.g. `col_span = u16::MAX`) cannot amplify into millions of cells/elements.
/// Far above any real document (Excel maxes at 16384 columns).
const MAX_TABLE_COLS: usize = 1024;

/// One drawable run on a line: its x offset within the content box, the krilla
/// glyphs, the resolved font, the size, the fill color, and the source text (for
/// the ToUnicode map that keeps the PDF text selectable).
#[derive(Clone)]
struct RunDraw {
    x: f32,
    glyphs: Vec<KrillaGlyph>,
    font: Font,
    size: f32,
    color: rgb::Color,
    text: Rc<str>,
}

/// A laid-out line: its advance height, the baseline offset from the line top,
/// its left indent (0 inside table cells; set for indented/list paragraphs), and
/// its runs.
#[derive(Clone)]
struct LineLayout {
    height: f32,
    baseline: f32,
    x_indent: f32,
    runs: Vec<RunDraw>,
}

/// A bordered table cell: its left edge + width (relative to the content origin),
/// its wrapped rich text lines, the background fill, and the vertical alignment.
#[derive(Clone)]
struct CellBox {
    x: f32,
    width: f32,
    lines: Vec<LineLayout>,
    shading: Option<rgb::Color>,
    valign: VCell,
}

/// One table row: its height and the cells across it (including empty cells where
/// a `row_span` from an earlier row covers a column).
#[derive(Clone)]
struct RowLayout {
    height: f32,
    cells: Vec<CellBox>,
}

/// A unit of block flow, paginated top-to-bottom. `Table` groups its rows (with the
/// header-row count) so pagination can repeat headers and split oversized rows;
/// `Row` is an individual placed row produced during pagination.
enum FlowItem {
    Gap(f32),
    Line(LineLayout),
    Row(RowLayout),
    Table {
        rows: Vec<RowLayout>,
        header_rows: usize,
    },
    Picture {
        image: PdfImage,
        w: f32,
        h: f32,
    },
}

/// Decode embedded image bytes into a krilla raster image, by MIME when known and
/// otherwise by magic-byte sniffing. Returns the image and its pixel dimensions,
/// or `None` for an unrecognized/undecodable format (so the renderer skips it
/// rather than panicking).
fn decode_image(bytes: &[u8], mime: Option<&str>) -> Option<(PdfImage, u32, u32)> {
    let data: Data = bytes.to_vec().into();
    let is_webp = bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP";
    let img = match mime {
        Some("image/png") => PdfImage::from_png(data, false),
        Some("image/jpeg") => PdfImage::from_jpeg(data, false),
        Some("image/gif") => PdfImage::from_gif(data, false),
        Some("image/webp") => PdfImage::from_webp(data, false),
        _ if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) => PdfImage::from_png(data, false),
        _ if bytes.starts_with(&[0xFF, 0xD8]) => PdfImage::from_jpeg(data, false),
        _ if bytes.starts_with(b"GIF8") => PdfImage::from_gif(data, false),
        _ if is_webp => PdfImage::from_webp(data, false),
        _ => return None,
    }
    .ok()?;
    let (w, h) = img.size();
    Some((img, w, h))
}

/// Decode a model image and size it to a [`FlowItem::Picture`] (96-dpi px → PDF
/// points, fit to the content box and a single page height, aspect preserved).
/// `None` if there are no bytes or the format is undecodable.
fn image_flow_item(img: &Image) -> Option<FlowItem> {
    let bytes = img.bytes.as_ref()?;
    let (image, wpx, hpx) = decode_image(bytes, img.mime.as_deref())?;
    let mut w = wpx as f32 * 0.75;
    let mut h = hpx as f32 * 0.75;
    // CONTENT_W and max_h are positive constants, so exceeding them implies > 0.
    if w > CONTENT_W {
        let s = CONTENT_W / w;
        w = CONTENT_W;
        h *= s;
    }
    let max_h = BOTTOM - TOP;
    if h > max_h {
        let s = max_h / h;
        h = max_h;
        w *= s;
    }
    (w > 0.0 && h > 0.0).then_some(FlowItem::Picture { image, w, h })
}

/// The system font stack, with Windows/Noto Korean faces preferred so Hangul
/// shapes even before fontique's automatic per-script fallback kicks in.
fn font_stack() -> FontFamily<'static> {
    FontFamily::List(Cow::Borrowed(&[
        FontFamilyName::Named(Cow::Borrowed("Malgun Gothic")),
        FontFamilyName::Named(Cow::Borrowed("Noto Sans CJK KR")),
        FontFamilyName::Named(Cow::Borrowed("Noto Sans KR")),
        FontFamilyName::Named(Cow::Borrowed("Arial")),
    ]))
}

/// The system stack with a named face tried first (for an authored `CharProps.font`),
/// then the Korean-capable fallbacks.
fn named_stack(name: &str) -> FontFamily<'static> {
    FontFamily::List(Cow::Owned(vec![
        FontFamilyName::Named(Cow::Owned(name.to_string())),
        FontFamilyName::Named(Cow::Borrowed("Malgun Gothic")),
        FontFamilyName::Named(Cow::Borrowed("Noto Sans CJK KR")),
        FontFamilyName::Named(Cow::Borrowed("Noto Sans KR")),
        FontFamilyName::Named(Cow::Borrowed("Arial")),
    ]))
}

/// The fill color of the `CharProps` range covering byte `pos` (default black).
/// Color is not a shaping property, so parley does not split runs on it; we look
/// it up per cluster and split draw segments where it changes.
fn color_at(ranges: &[(usize, usize, CharProps)], pos: usize) -> rgb::Color {
    for (s, e, p) in ranges {
        if pos >= *s && pos < *e {
            return p
                .color
                .map(|c| rgb::Color::new(c.r, c.g, c.b))
                .unwrap_or(rgb::Color::new(0, 0, 0));
        }
    }
    rgb::Color::new(0, 0, 0)
}

fn heading_size(level: Option<u8>) -> f32 {
    match level {
        Some(1) => 20.0,
        Some(2) => 17.0,
        Some(3) => 15.0,
        Some(4) => 13.5,
        Some(_) => 12.5,
        None => 11.0,
    }
}

/// Shape a styled text string into positioned lines at a given wrap `width`.
#[allow(clippy::too_many_arguments)]
fn shape(
    text: &str,
    ranges: &[(usize, usize, CharProps)],
    heading_level: Option<u8>,
    align: Alignment,
    width: f32,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) -> Vec<LineLayout> {
    let base_size = heading_size(heading_level);
    let heading = heading_level.is_some();

    let mut builder = layout_cx.ranged_builder(font_cx, text, 1.0, false);
    builder.push_default(StyleProperty::Brush(rgb::Color::new(0, 0, 0)));
    builder.push_default(StyleProperty::FontFamily(font_stack()));
    builder.push_default(StyleProperty::FontSize(base_size));
    builder.push_default(StyleProperty::LineHeight(
        parley::style::LineHeight::FontSizeRelative(1.35),
    ));
    if heading {
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(700.0)));
    }
    for (s, e, props) in ranges {
        if props.bold && !heading {
            builder.push(StyleProperty::FontWeight(FontWeight::new(700.0)), *s..*e);
        }
        if props.italic {
            builder.push(StyleProperty::FontStyle(FontStyle::Italic), *s..*e);
        }
        if props.underline {
            builder.push(StyleProperty::Underline(true), *s..*e);
        }
        if props.strike {
            builder.push(StyleProperty::Strikethrough(true), *s..*e);
        }
        // Authored character size (half-points → points) overrides the base size.
        if let Some(half) = props.size_half_pt {
            if half > 0 {
                builder.push(StyleProperty::FontSize(half as f32 / 2.0), *s..*e);
            }
        }
        // Authored font family, tried before the Korean-capable fallbacks.
        if let Some(name) = &props.font {
            if !name.is_empty() {
                builder.push(StyleProperty::FontFamily(named_stack(name)), *s..*e);
            }
        }
    }

    let mut layout = builder.build(text);
    layout.break_all_lines(Some(width.max(1.0)));
    layout.align(align, Default::default());

    let text_rc: Rc<str> = Rc::from(text);
    let mut out = Vec::new();
    for line in layout.lines() {
        let m = line.metrics();
        let baseline = m.ascent + m.leading * 0.5;
        let height = m.line_height;
        let mut runs: Vec<RunDraw> = Vec::new();
        // `offset` is the line's alignment shift (0 for Start/left).
        let mut x_cursor = m.offset;
        for run in line.runs() {
            let run_x = x_cursor;
            let font = run.font().clone();
            let (font_data, id) = font.data.into_raw_parts();
            // A face parley can shape but krilla cannot ingest (bitmap/COLR/odd
            // index) makes `Font::new` return `None` — skip the run rather than
            // panic, honoring the crate's panic-free contract.
            let krilla_font = match font_cache.get(&id) {
                Some(f) => f.clone(),
                None => match Font::new(font_data.into(), font.index) {
                    Some(f) => {
                        font_cache.insert(id, f.clone());
                        f
                    }
                    None => continue,
                },
            };
            let font_size = run.font_size();
            let mut glyphs: Vec<KrillaGlyph> = Vec::new();
            // Color can change within a single (uniformly-shaped) parley run, so
            // accumulate glyphs into per-color segments, flushing on each change.
            let mut seg_color = rgb::Color::new(0, 0, 0);
            let mut seg_x = run_x;
            let mut started = false;
            for cluster in run.visual_clusters() {
                if cluster.is_ligature_continuation() {
                    if let Some(g) = glyphs.last_mut() {
                        g.text_range.end = cluster.text_range().end;
                    }
                    continue;
                }
                let c = color_at(ranges, cluster.text_range().start);
                if started && c != seg_color && !glyphs.is_empty() {
                    runs.push(RunDraw {
                        x: seg_x,
                        glyphs: std::mem::take(&mut glyphs),
                        font: krilla_font.clone(),
                        size: font_size,
                        color: seg_color,
                        text: text_rc.clone(),
                    });
                    seg_x = x_cursor;
                }
                seg_color = c;
                started = true;
                for glyph in cluster.glyphs() {
                    glyphs.push(KrillaGlyph::new(
                        GlyphId::new(glyph.id),
                        glyph.advance / font_size,
                        glyph.x / font_size,
                        glyph.y / font_size,
                        0.0,
                        cluster.text_range(),
                        None,
                    ));
                    x_cursor += glyph.advance;
                }
            }
            if !glyphs.is_empty() {
                runs.push(RunDraw {
                    x: seg_x,
                    glyphs,
                    font: krilla_font,
                    size: font_size,
                    color: seg_color,
                    text: text_rc.clone(),
                });
            }
        }
        out.push(LineLayout {
            height,
            baseline,
            x_indent: 0.0,
            runs,
        });
    }
    out
}

/// Per-document ordered-list counters (levels 0..=8). Bullets and reader-captured
/// labels need no counter; an authored ordered item without a label is numbered
/// here.
#[derive(Default)]
struct ListState {
    counters: [u32; 9],
}

impl ListState {
    /// The marker for a list item, advancing/resetting the ordered counters.
    /// Prefers the reader's captured label; otherwise synthesizes `1.`/`2.`… for
    /// ordered lists or a per-level bullet glyph.
    fn marker(&mut self, list: &ListInfo) -> String {
        let lvl = (list.level as usize).min(8);
        if list.ordered {
            for c in self.counters.iter_mut().skip(lvl + 1) {
                *c = 0;
            }
            self.counters[lvl] += 1;
        }
        if !list.label.trim().is_empty() {
            return list.label.trim().to_string();
        }
        if list.ordered {
            format!("{}.", self.counters[lvl])
        } else {
            match lvl % 3 {
                0 => "•",
                1 => "◦",
                _ => "▪",
            }
            .to_string()
        }
    }
}

/// Lay out one paragraph into flow items, with an optional list `marker` and the
/// paragraph's left/right indent (list level adds a per-level indent).
fn layout_paragraph(
    p: &Paragraph,
    out: &mut Vec<FlowItem>,
    marker: Option<&str>,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) {
    let list_level = p.props.list.as_ref().map(|l| l.level).unwrap_or(0) as f32;
    let left = p.props.indent.left_pt.unwrap_or(0.0).max(0.0) + list_level * LIST_INDENT;
    let right = p.props.indent.right_pt.unwrap_or(0.0).max(0.0);
    let wrap_w = (CONTENT_W - left - right).max(20.0);

    let mut text = String::new();
    let mut ranges: Vec<(usize, usize, CharProps)> = Vec::new();
    let mut images: Vec<&Image> = Vec::new();
    if let Some(m) = marker {
        if !m.is_empty() {
            text.push_str(m);
            text.push(' ');
            ranges.push((0, text.len(), CharProps::default()));
        }
    }
    for r in &p.runs {
        // The reader carries images as inline runs (Run.image); flow them as
        // block pictures after the paragraph's text.
        if let Some(img) = &r.image {
            images.push(img);
        }
        if r.text.is_empty() {
            continue;
        }
        let s = text.len();
        text.push_str(&r.text);
        ranges.push((s, text.len(), r.props.clone()));
    }
    if !text.trim().is_empty() {
        let align = match p.props.align {
            Align::Left => Alignment::Start,
            Align::Center => Alignment::Center,
            Align::Right => Alignment::Right,
            Align::Justify => Alignment::Justify,
        };
        for mut line in shape(
            &text,
            &ranges,
            p.props.heading_level,
            align,
            wrap_w,
            font_cx,
            layout_cx,
            font_cache,
        ) {
            line.x_indent = left;
            out.push(FlowItem::Line(line));
        }
    }
    for img in images {
        if let Some(item) = image_flow_item(img) {
            out.push(FlowItem::Gap(PARA_GAP));
            out.push(item);
        }
    }
}

/// A cell placed on the reconstructed grid: its starting column, column span, and
/// the source cell (`None` = a `row_span` continuation slot, drawn as an empty box).
struct PlacedCell<'a> {
    col: usize,
    span: usize,
    cell: Option<&'a Cell>,
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
    for row in &t.rows {
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
                let rs = (c.row_span.max(1) as usize).min(MAX_TABLE_COLS);
                placed.push(PlacedCell {
                    col,
                    span,
                    cell: Some(c),
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

/// The unwrapped (single-line) width of a string at body size — used to size
/// table columns to their content.
fn natural_width(
    text: &str,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
) -> f32 {
    if text.trim().is_empty() {
        return 0.0;
    }
    let mut b = layout_cx.ranged_builder(font_cx, text, 1.0, false);
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
/// Nested tables/images in a cell are not laid out as grids — only their text was
/// ever surfaced — so they contribute nothing here, matching [`Cell::text`].
fn shape_cell(
    cell: &Cell,
    inner_w: f32,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) -> Vec<LineLayout> {
    let mut lines = Vec::new();
    for b in &cell.blocks {
        if let Block::Paragraph(p) = b {
            let mut text = String::new();
            let mut ranges: Vec<(usize, usize, CharProps)> = Vec::new();
            for r in &p.runs {
                if r.text.is_empty() {
                    continue;
                }
                let s = text.len();
                text.push_str(&r.text);
                ranges.push((s, text.len(), r.props.clone()));
            }
            if text.trim().is_empty() {
                continue;
            }
            let align = match p.props.align {
                Align::Left => Alignment::Start,
                Align::Center => Alignment::Center,
                Align::Right => Alignment::Right,
                Align::Justify => Alignment::Justify,
            };
            lines.extend(shape(
                &text,
                &ranges,
                p.props.heading_level,
                align,
                inner_w,
                font_cx,
                layout_cx,
                font_cache,
            ));
        }
    }
    lines
}

/// Lay out a table into one [`FlowItem::Row`] per row. Column widths come from the
/// model's authored `col_widths_pct` when present; otherwise columns are sized to
/// their content (natural widths scaled to fill the content box), so a narrow
/// label column and a wide value column read correctly instead of being equal.
fn layout_table(
    t: &Table,
    out: &mut Vec<FlowItem>,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) {
    let (grid, ncols) = reconstruct_grid(t);

    // Column edges: honor authored percentages when they match the grid, else
    // size to content (min 20pt/col) and scale to fill the content width.
    let mut col_x = vec![0.0_f32; ncols + 1];
    if t.col_widths_pct.len() == ncols && t.col_widths_pct.iter().all(|w| *w > 0.0) {
        let sum: f32 = t.col_widths_pct.iter().sum();
        for c in 0..ncols {
            col_x[c + 1] = col_x[c] + CONTENT_W * (t.col_widths_pct[c] / sum);
        }
    } else {
        let mut col_nat = vec![20.0_f32; ncols];
        for placed_row in &grid {
            for pc in placed_row {
                if let Some(c) = pc.cell {
                    let txt = c.text().replace('\n', " ");
                    let per = (natural_width(&txt, font_cx, layout_cx) + 2.0 * CELL_PAD)
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
        let scale = if total > 0.0 { CONTENT_W / total } else { 1.0 };
        for c in 0..ncols {
            col_x[c + 1] = col_x[c] + col_nat[c] * scale;
        }
    }

    // Pass 2: shape each cell richly at its column width and build the rows.
    let mut rows: Vec<RowLayout> = Vec::with_capacity(grid.len());
    for placed_row in grid {
        let mut cells = Vec::with_capacity(placed_row.len());
        let mut row_h = 0.0_f32;
        for pc in placed_row {
            let end = (pc.col + pc.span).min(ncols);
            let x = col_x[pc.col];
            let width = col_x[end] - x;
            let (lines, shading, valign) = match pc.cell {
                Some(c) => {
                    let lines = shape_cell(
                        c,
                        (width - 2.0 * CELL_PAD).max(1.0),
                        font_cx,
                        layout_cx,
                        font_cache,
                    );
                    let shading = c.shading.map(|s| rgb::Color::new(s.r, s.g, s.b));
                    (lines, shading, c.valign)
                }
                None => (Vec::new(), None, VCell::Top),
            };
            let content_h: f32 = lines.iter().map(|l| l.height).sum();
            row_h = row_h.max(content_h + 2.0 * CELL_PAD);
            cells.push(CellBox {
                x,
                width,
                lines,
                shading,
                valign,
            });
        }
        // A minimum row height so empty rows still draw a band.
        row_h = row_h.max(14.0);
        rows.push(RowLayout {
            height: row_h,
            cells,
        });
    }
    let header_rows = t.header_rows.min(rows.len());
    out.push(FlowItem::Table { rows, header_rows });
}

/// Split a row into a fragment that fits `avail` points of height and the leftover
/// rest, by partitioning each cell's lines. At least one line is always kept in
/// the fragment so progress is guaranteed even for a line taller than a page.
fn split_row(row: RowLayout, avail: f32) -> (RowLayout, Option<RowLayout>) {
    let budget = (avail - 2.0 * CELL_PAD).max(0.0);
    let mut frag_cells = Vec::with_capacity(row.cells.len());
    let mut rest_cells = Vec::with_capacity(row.cells.len());
    let mut any_rest = false;
    for cell in row.cells {
        let CellBox {
            x,
            width,
            shading,
            valign,
            lines,
        } = cell;
        let mut used = 0.0_f32;
        let mut head = Vec::new();
        let mut tail = Vec::new();
        for line in lines {
            if tail.is_empty() && (head.is_empty() || used + line.height <= budget) {
                used += line.height;
                head.push(line);
            } else {
                tail.push(line);
            }
        }
        if !tail.is_empty() {
            any_rest = true;
        }
        frag_cells.push(CellBox {
            x,
            width,
            shading,
            valign,
            lines: head,
        });
        rest_cells.push(CellBox {
            x,
            width,
            shading,
            valign,
            lines: tail,
        });
    }
    let frag = RowLayout {
        height: avail,
        cells: frag_cells,
    };
    if any_rest {
        let rest_h = rest_cells
            .iter()
            .map(|c| c.lines.iter().map(|l| l.height).sum::<f32>())
            .fold(0.0_f32, f32::max)
            + 2.0 * CELL_PAD;
        let rest = RowLayout {
            height: rest_h.max(14.0),
            cells: rest_cells,
        };
        (frag, Some(rest))
    } else {
        (frag, None)
    }
}

fn collect_blocks(
    blocks: &[Block],
    out: &mut Vec<FlowItem>,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) {
    let mut lists = ListState::default();
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                // A heading suppresses list marking, mirroring the writer.
                let marker = match (&p.props.list, p.props.heading_level) {
                    (Some(list), None) => Some(lists.marker(list)),
                    _ => None,
                };
                if let Some(before) = p.props.spacing.before_pt.filter(|b| *b > 0.0) {
                    out.push(FlowItem::Gap(before));
                }
                layout_paragraph(p, out, marker.as_deref(), font_cx, layout_cx, font_cache);
                let after = p
                    .props
                    .spacing
                    .after_pt
                    .filter(|a| *a > 0.0)
                    .unwrap_or(PARA_GAP);
                out.push(FlowItem::Gap(after));
            }
            Block::Table(t) => {
                layout_table(t, out, font_cx, layout_cx, font_cache);
                out.push(FlowItem::Gap(PARA_GAP));
            }
            Block::Image(img) => {
                if let Some(item) = image_flow_item(img) {
                    out.push(item);
                    out.push(FlowItem::Gap(PARA_GAP));
                }
            }
        }
    }
}

/// Fill an axis-aligned rectangle in a solid color.
fn fill_rect_color(surface: &mut Surface<'_>, x: f32, y: f32, w: f32, h: f32, color: rgb::Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x, y);
    pb.line_to(x + w, y);
    pb.line_to(x + w, y + h);
    pb.line_to(x, y + h);
    pb.close();
    if let Some(path) = pb.finish() {
        surface.set_fill(Some(Fill {
            paint: color.into(),
            rule: FillRule::NonZero,
            opacity: NormalizedF32::ONE,
        }));
        surface.draw_path(&path);
    }
}

/// Fill an axis-aligned rectangle in solid black (used for thin table borders).
fn fill_rect(surface: &mut Surface<'_>, x: f32, y: f32, w: f32, h: f32) {
    fill_rect_color(surface, x, y, w, h, rgb::Color::new(0, 0, 0));
}

/// Draw a cell border (four thin edges) around `(x, y, w, h)`.
fn draw_border(surface: &mut Surface<'_>, x: f32, y: f32, w: f32, h: f32) {
    fill_rect(surface, x, y, w, BORDER); // top
    fill_rect(surface, x, y + h - BORDER, w, BORDER); // bottom
    fill_rect(surface, x, y, BORDER, h); // left
    fill_rect(surface, x + w - BORDER, y, BORDER, h); // right
}

/// Draw a run's glyphs at an absolute baseline position, in the run's color.
fn draw_run(surface: &mut Surface<'_>, run: RunDraw, x_abs: f32, baseline_y: f32) {
    surface.set_fill(Some(Fill {
        paint: run.color.into(),
        rule: FillRule::NonZero,
        opacity: NormalizedF32::ONE,
    }));
    surface.draw_glyphs(
        Point::from_xy(x_abs + run.x, baseline_y),
        &run.glyphs,
        run.font,
        &run.text,
        run.size,
        false,
    );
}

type Pages = Vec<Vec<(f32, FlowItem)>>;

fn page_nonempty(pages: &Pages) -> bool {
    pages.last().map(|p| !p.is_empty()).unwrap_or(false)
}

/// Place an item at the current `y` on the last page, then advance `y`.
fn place_item(pages: &mut Pages, y: &mut f32, item: FlowItem, h: f32) {
    if let Some(p) = pages.last_mut() {
        p.push((*y, item));
    }
    *y += h;
}

/// Break to a fresh page if `h` won't fit the remaining space on a non-empty page.
fn ensure(pages: &mut Pages, y: &mut f32, h: f32) {
    if *y + h > BOTTOM && page_nonempty(pages) {
        pages.push(Vec::new());
        *y = TOP;
    }
}

/// Re-place the header rows (clones) at the top of the current page.
fn repeat_headers(pages: &mut Pages, y: &mut f32, headers: &[RowLayout]) {
    for h in headers {
        let hr = h.clone();
        let hh = hr.height;
        place_item(pages, y, FlowItem::Row(hr), hh);
    }
}

/// Place one row, breaking pages as needed: a row that fits a fresh page is moved
/// whole (with the header rows repeated); a row taller than a page is split across
/// pages at line boundaries. `is_header` rows are never themselves preceded by a
/// header repeat.
fn place_row(
    pages: &mut Pages,
    y: &mut f32,
    mut row: RowLayout,
    headers: &[RowLayout],
    is_header: bool,
) {
    let mut on_fresh = !page_nonempty(pages);
    loop {
        let avail = BOTTOM - *y;
        if row.height <= avail {
            let h = row.height;
            place_item(pages, y, FlowItem::Row(row), h);
            return;
        }
        if !on_fresh {
            // Move the whole row to a fresh page and repeat headers.
            pages.push(Vec::new());
            *y = TOP;
            if !is_header {
                repeat_headers(pages, y, headers);
            }
            on_fresh = true;
            continue;
        }
        // On a fresh page (after any headers) and still too tall: split.
        let (frag, rest) = split_row(row, BOTTOM - *y);
        let fh = frag.height;
        place_item(pages, y, FlowItem::Row(frag), fh);
        pages.push(Vec::new());
        *y = TOP;
        if !is_header {
            repeat_headers(pages, y, headers);
        }
        match rest {
            Some(r) => row = r,
            None => return,
        }
    }
}

/// Paginate a table: place every row, repeating the header rows after each break.
fn place_table(pages: &mut Pages, y: &mut f32, rows: Vec<RowLayout>, header_rows: usize) {
    let headers: Vec<RowLayout> = rows.iter().take(header_rows).cloned().collect();
    for (i, row) in rows.into_iter().enumerate() {
        place_row(pages, y, row, &headers, i < header_rows);
    }
}

/// Render a [`DocModel`] to a single-column A4 PDF.
pub(crate) fn to_pdf(model: &DocModel) -> Vec<u8> {
    let mut font_cx = FontContext::default();
    let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
    let mut font_cache: HashMap<u64, Font> = HashMap::new();

    let mut items: Vec<FlowItem> = Vec::new();
    collect_blocks(
        &model.blocks,
        &mut items,
        &mut font_cx,
        &mut layout_cx,
        &mut font_cache,
    );

    // Paginate: flow items top-to-bottom onto A4 pages. Tables repeat their header
    // rows after each break and split rows taller than a page across pages.
    let mut pages: Pages = vec![Vec::new()];
    let mut y = TOP;
    for item in items {
        match item {
            FlowItem::Gap(g) => y += g,
            FlowItem::Line(l) => {
                let h = l.height;
                ensure(&mut pages, &mut y, h);
                place_item(&mut pages, &mut y, FlowItem::Line(l), h);
            }
            FlowItem::Picture { image, w, h } => {
                ensure(&mut pages, &mut y, h);
                place_item(&mut pages, &mut y, FlowItem::Picture { image, w, h }, h);
            }
            FlowItem::Table { rows, header_rows } => {
                place_table(&mut pages, &mut y, rows, header_rows);
            }
            // Rows reach pagination only inside a Table; place defensively.
            FlowItem::Row(r) => {
                let h = r.height;
                ensure(&mut pages, &mut y, h);
                place_item(&mut pages, &mut y, FlowItem::Row(r), h);
            }
        }
    }

    // Emit.
    let mut document = PdfDoc::new();
    for page_items in pages {
        let Some(settings) = PageSettings::from_wh(PAGE_W, PAGE_H) else {
            continue;
        };
        let mut page = document.start_page_with(settings);
        let mut surface = page.surface();
        for (top, item) in page_items {
            match item {
                FlowItem::Gap(_) | FlowItem::Table { .. } => {}
                FlowItem::Picture { image, w, h } => {
                    // Center horizontally within the content box.
                    let x = MARGIN + ((CONTENT_W - w) * 0.5).max(0.0);
                    if let Some(sz) = Size::from_wh(w, h) {
                        surface.push_transform(&Transform::from_translate(x, top));
                        surface.draw_image(image, sz);
                        surface.pop();
                    }
                }
                FlowItem::Line(line) => {
                    let baseline = top + line.baseline;
                    let x0 = MARGIN + line.x_indent;
                    for run in line.runs {
                        draw_run(&mut surface, run, x0, baseline);
                    }
                }
                FlowItem::Row(row) => {
                    for cell in row.cells {
                        let cx = MARGIN + cell.x;
                        if let Some(fill) = cell.shading {
                            fill_rect_color(&mut surface, cx, top, cell.width, row.height, fill);
                        }
                        draw_border(&mut surface, cx, top, cell.width, row.height);
                        // Vertical alignment within the cell band.
                        let content_h: f32 = cell.lines.iter().map(|l| l.height).sum();
                        let avail = row.height - 2.0 * CELL_PAD;
                        let off = match cell.valign {
                            VCell::Top => 0.0,
                            VCell::Center => ((avail - content_h) * 0.5).max(0.0),
                            VCell::Bottom => (avail - content_h).max(0.0),
                        };
                        let mut ly = top + CELL_PAD + off;
                        for line in cell.lines {
                            let baseline = ly + line.baseline;
                            let lh = line.height;
                            for run in line.runs {
                                draw_run(&mut surface, run, cx + CELL_PAD, baseline);
                            }
                            ly += lh;
                        }
                    }
                }
            }
        }
        surface.finish();
        page.finish();
    }
    document.finish().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::model::{Block, Cell, DocModel, ParaProps, Paragraph, Row, Run, Table};

    fn para(text: &str, heading: Option<u8>) -> Block {
        Block::Paragraph(Paragraph {
            props: ParaProps {
                heading_level: heading,
                ..ParaProps::default()
            },
            runs: vec![Run {
                text: text.to_string(),
                ..Run::default()
            }],
        })
    }

    fn cell(text: &str) -> Cell {
        Cell {
            blocks: vec![para(text, None)],
            ..Cell::default()
        }
    }

    #[test]
    fn renders_a_valid_pdf() {
        let model = DocModel {
            blocks: vec![
                para("제목 하나", Some(1)),
                para("본문 문단 with mixed English and 한글 text.", None),
            ],
            ..DocModel::default()
        };
        let pdf = super::to_pdf(&model);
        assert!(pdf.starts_with(b"%PDF"), "output is not a PDF");
        assert!(
            pdf.len() > 500,
            "PDF unexpectedly small: {} bytes",
            pdf.len()
        );
    }

    #[test]
    fn renders_a_table_grid() {
        let table = Table {
            rows: vec![
                Row {
                    cells: vec![cell("항목"), cell("내용")],
                },
                Row {
                    cells: vec![cell("가격"), cell("1,000원")],
                },
            ],
            header_rows: 1,
            ..Default::default()
        };
        let model = DocModel {
            blocks: vec![para("표 테스트", Some(2)), Block::Table(table)],
            ..DocModel::default()
        };
        let pdf = super::to_pdf(&model);
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 800);
    }

    #[test]
    fn renders_rich_runs_without_panicking() {
        use crate::model::{CharProps, Color};
        let model = DocModel {
            blocks: vec![Block::Paragraph(Paragraph {
                props: ParaProps::default(),
                runs: vec![
                    Run {
                        text: "검정 ".to_string(),
                        ..Run::default()
                    },
                    Run {
                        text: "빨강 큰글씨".to_string(),
                        props: CharProps {
                            color: Some(Color {
                                r: 0xC0,
                                g: 0,
                                b: 0,
                            }),
                            size_half_pt: Some(36),
                            bold: true,
                            font: Some("Malgun Gothic".to_string()),
                            ..CharProps::default()
                        },
                        ..Run::default()
                    },
                ],
            })],
            ..DocModel::default()
        };
        let pdf = super::to_pdf(&model);
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 500);
    }

    #[test]
    fn renders_rich_shaded_table_without_panicking() {
        use crate::model::{CharProps, Color, VCell};
        let navy = Color {
            r: 0x1F,
            g: 0x38,
            b: 0x64,
        };
        let white = Color {
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
        };
        let hdr = Cell {
            blocks: vec![Block::Paragraph(Paragraph {
                props: ParaProps::default(),
                runs: vec![Run {
                    text: "항목".to_string(),
                    props: CharProps {
                        bold: true,
                        color: Some(white),
                        ..CharProps::default()
                    },
                    ..Run::default()
                }],
            })],
            shading: Some(navy),
            valign: VCell::Center,
            ..Cell::default()
        };
        let table = Table {
            rows: vec![
                Row {
                    cells: vec![hdr, cell("값")],
                },
                Row {
                    cells: vec![cell("가격"), cell("1,000원")],
                },
            ],
            header_rows: 1,
            col_widths_pct: vec![0.3, 0.7],
        };
        let pdf = super::to_pdf(&DocModel {
            blocks: vec![Block::Table(table)],
            ..DocModel::default()
        });
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 800);
    }

    #[test]
    fn renders_embedded_image() {
        use crate::model::Image;
        // A 4×3 PNG (solid navy), generated and frozen as a fixture.
        const TINY_PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x3B, 0x96, 0x39, 0x91, 0x00, 0x00, 0x00, 0x13, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x94, 0xB7, 0x48, 0x61, 0x80, 0x01, 0x26, 0x38, 0x0B, 0x9D, 0x03, 0x00,
            0x1B, 0x5E, 0x00, 0xC1, 0xBF, 0x92, 0xAB, 0x14, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
            0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        // Both representations: a block image and an inline-run image (what the
        // reader produces) must render.
        let model = DocModel {
            blocks: vec![
                Block::Image(Image {
                    bytes: Some(TINY_PNG.to_vec()),
                    mime: Some("image/png".to_string()),
                    ..Image::default()
                }),
                Block::Paragraph(Paragraph {
                    props: ParaProps::default(),
                    runs: vec![Run {
                        image: Some(Image {
                            bytes: Some(TINY_PNG.to_vec()),
                            mime: None, // force magic-byte sniffing
                            ..Image::default()
                        }),
                        ..Run::default()
                    }],
                }),
            ],
            ..DocModel::default()
        };
        let pdf = super::to_pdf(&model);
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 500);
        // An undecodable blob must be skipped, not panic.
        let bad = DocModel {
            blocks: vec![Block::Image(Image {
                bytes: Some(vec![1, 2, 3, 4]),
                mime: Some("image/png".to_string()),
                ..Image::default()
            })],
            ..DocModel::default()
        };
        assert!(super::to_pdf(&bad).starts_with(b"%PDF"));
    }

    #[test]
    fn list_state_numbers_and_resets_levels() {
        use crate::model::ListInfo;
        let mut s = super::ListState::default();
        let ol = |level: u8| ListInfo {
            level,
            ordered: true,
            label: String::new(),
        };
        let ul = |level: u8| ListInfo {
            level,
            ordered: false,
            label: String::new(),
        };
        assert_eq!(s.marker(&ol(0)), "1.");
        assert_eq!(s.marker(&ol(0)), "2.");
        assert_eq!(s.marker(&ul(1)), "◦"); // nested bullet doesn't bump level 0
        assert_eq!(s.marker(&ol(1)), "1."); // first ordered at level 1
        assert_eq!(s.marker(&ol(0)), "3."); // level 0 resumes
        assert_eq!(s.marker(&ol(1)), "1."); // level 1 was reset by the level-0 item
                                            // A reader-captured label is preferred verbatim.
        assert_eq!(
            s.marker(&ListInfo {
                level: 0,
                ordered: true,
                label: "가.".to_string()
            }),
            "가."
        );
    }

    #[test]
    fn renders_lists_and_indent_without_panicking() {
        use crate::model::{Indent, ListInfo};
        let item = |level: u8, ordered: bool, t: &str| {
            Block::Paragraph(Paragraph {
                props: ParaProps {
                    list: Some(ListInfo {
                        level,
                        ordered,
                        label: String::new(),
                    }),
                    ..ParaProps::default()
                },
                runs: vec![Run {
                    text: t.to_string(),
                    ..Run::default()
                }],
            })
        };
        let indented = Block::Paragraph(Paragraph {
            props: ParaProps {
                indent: Indent {
                    left_pt: Some(36.0),
                    ..Indent::default()
                },
                ..ParaProps::default()
            },
            runs: vec![Run {
                text: "들여쓰기된 문단".to_string(),
                ..Run::default()
            }],
        });
        let model = DocModel {
            blocks: vec![
                item(0, true, "첫째"),
                item(1, false, "하위 항목"),
                item(0, true, "둘째"),
                indented,
            ],
            ..DocModel::default()
        };
        let pdf = super::to_pdf(&model);
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 600);
    }

    #[test]
    fn multi_page_table_and_oversized_row_terminate() {
        // A table whose rows exceed a page (header repeat path) plus one row with a
        // cell taller than several pages (the split path) must render to a bounded,
        // valid multi-page PDF without hanging.
        let hdr = Cell {
            blocks: vec![para("머리글", None)],
            is_header: true,
            ..Cell::default()
        };
        let mut rows = vec![Row {
            cells: vec![hdr, cell("값")],
        }];
        for i in 0..80 {
            rows.push(Row {
                cells: vec![cell(&format!("행 {i}")), cell("내용")],
            });
        }
        // A single row with a very tall cell (300 paragraphs ⇒ 300+ lines).
        let tall_blocks: Vec<Block> = (0..300).map(|n| para(&format!("줄 {n}"), None)).collect();
        rows.push(Row {
            cells: vec![
                Cell {
                    blocks: tall_blocks,
                    ..Cell::default()
                },
                cell("끝"),
            ],
        });
        let model = DocModel {
            blocks: vec![Block::Table(Table {
                rows,
                header_rows: 1,
                ..Default::default()
            })],
            ..DocModel::default()
        };
        let pdf = super::to_pdf(&model);
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() < 5_000_000, "unexpectedly large: {}", pdf.len());
    }

    #[test]
    fn empty_model_renders_without_panicking() {
        let pdf = super::to_pdf(&DocModel::default());
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn giant_span_table_renders_bounded() {
        // A hostile col_span must be clamped so the renderer can't amplify into
        // millions of columns / panic.
        let model = DocModel {
            blocks: vec![Block::Table(Table {
                rows: vec![Row {
                    cells: vec![Cell {
                        blocks: vec![para("x", None)],
                        col_span: u16::MAX,
                        row_span: 1,
                        is_header: false,
                        ..Default::default()
                    }],
                }],
                header_rows: 0,
                ..Default::default()
            })],
            ..DocModel::default()
        };
        let pdf = super::to_pdf(&model);
        assert!(pdf.starts_with(b"%PDF"));
        assert!(
            pdf.len() < 3_000_000,
            "giant span amplified PDF to {} bytes",
            pdf.len()
        );
    }
}
