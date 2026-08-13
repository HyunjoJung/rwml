//! Native typesetting renderer: `DocModel` → A4 **PDF** via `parley` (layout,
//! shaping, Korean/CJK line-breaking + font fallback) and `krilla` (PDF emit with
//! subsetted embedded fonts and selectable text). Behind the `render` feature.
//!
//! Pipeline: blocks are laid out into a stream of flow items — text lines and
//! table rows — which are flowed top-to-bottom onto fixed A4 pages, then each
//! page's glyph runs and table borders are drawn with krilla. A table that spans
//! pages repeats its header rows after each break. Opened DOCX rows may split at
//! legal cell line boundaries unless effective `w:cantSplit` from direct row
//! properties or a resolved table-style chain keeps a fitting row together.
//! Recursively flattened nested rows retain the same protected boundary, while
//! an over-tall row still splits to guarantee progress.
//! Tables are rendered as a
//! real grid: columns are reconstructed
//! (including `col_span`/`row_span` placement), sized to authored `col_widths_pct`
//! or to content, then bordered; cells carry rich per-run text (bold/italic/color/
//! size/font), background shading, and vertical alignment. Images (block-level
//! and inline) are decoded and drawn as raster pictures; model-backed clockwise
//! rotation contributes to their fitted visual bounds and pagination.
//!
//! Fonts come from the system font collection (parley's default `FontContext`),
//! so Korean renders when a Hangul-capable face is installed. For headless/server
//! use without system CJK fonts, a caller can register its own font bytes via
//! [`crate::render_pdf_with_fonts`] (the renderer does not embed a multi-megabyte
//! CJK font into the crate; install one — e.g. Noto Sans CJK — or supply it).

use std::borrow::Cow;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::rc::Rc;

use krilla::action::LinkAction;
use krilla::annotation::{Annotation, LinkAnnotation, Target};
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image as PdfImage;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla::{Data, Document as PdfDoc};
use parley::layout::{Alignment, IndentOptions};
use parley::style::{FontFamily, FontFamilyName, FontStyle, FontWeight, StyleProperty};
use parley::{FontContext, Layout, LayoutContext};

use crate::model::{
    Align, Block, Cell, CellMargins, CharProps, Chart, ChartKind, ChartShape, Color, DocModel,
    FieldRole, Image, ListInfo, PageSetup, PaginationHint, ParaProps, Paragraph, Run,
    SectionBreakKind, SectionSetup, Spacing, TabAlignment, TabStop, Table, TableBorderSide,
    TableCellNestedPaginationHints, TableCellPaginationHints, TableCellTabStopHints,
    TablePaginationHints, TableRowPaginationHint, VCell, VertAlign,
};
use crate::report::{self, FeatureInventory, RenderReport, RenderWarning, RenderedPdf};
use crate::{Error, Result};
use crate::{FieldKind, FloatingShape, ShapePosition};

// A4 fallback page geometry, in PDF points (used when the model has no page setup).
const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 56.0;

/// Per-document page geometry in PDF points, derived from the model's `PageSetup`
/// (so Letter, A3, custom margins, and landscape all render at the right size
/// instead of a fixed A4). Replaces the former page-size constants.
#[derive(Clone, Copy)]
struct Geom {
    page_w: f32,
    page_h: f32,
    left: f32,
    right: f32,
    top_m: f32,
    bottom_m: f32,
}

impl Geom {
    fn from_setup(p: &PageSetup) -> Geom {
        let page_w = if p.width_pt > 72.0 {
            p.width_pt
        } else {
            PAGE_W
        };
        let page_h = if p.height_pt > 72.0 {
            p.height_pt
        } else {
            PAGE_H
        };
        // Clamp each side so the content box stays positive even on odd margins.
        let max_h = (page_w / 2.0 - 20.0).max(0.0);
        let max_v = (page_h / 2.0 - 20.0).max(0.0);
        let pick = |v: f32, dflt: f32, max: f32| (if v > 0.0 { v } else { dflt }).min(max);
        Geom {
            page_w,
            page_h,
            left: pick(p.left(), MARGIN, max_h),
            right: pick(p.right(), MARGIN, max_h),
            top_m: pick(p.top(), MARGIN, max_v),
            bottom_m: pick(p.bottom(), MARGIN, max_v),
        }
    }
    fn content_w(&self) -> f32 {
        (self.page_w - self.left - self.right).max(20.0)
    }
    fn top(&self) -> f32 {
        self.top_m
    }
    fn bottom(&self) -> f32 {
        self.page_h - self.bottom_m
    }

    fn with_content_width(self, width: f32) -> Self {
        let width = width.clamp(MIN_COLUMN_WIDTH_PT, self.content_w());
        Self {
            right: (self.page_w - self.left - width).max(0.0),
            ..self
        }
    }
}

const PARA_GAP: f32 = 6.0;
const CELL_PAD: f32 = 3.0;
const BORDER: f32 = 0.4;
const MAX_TABLE_BORDER_SIZE_EIGHTHS: u16 = 96;
/// Left indent added per list nesting level, in points.
const LIST_INDENT: f32 = 18.0;
/// Max nesting depth for tables-in-cells flattened by `shape_cell` (panic-free
/// bound against pathologically nested tables).
const MAX_CELL_DEPTH: u32 = 32;
/// Max laid-out lines kept for a single table cell. A cell taller than ~78 pages is not a
/// real document, but an unbounded line count makes the page-split paginator O(L²) (it peels
/// one page-worth per `split_row` pass). Far above any real cell; bounds the worst case.
const MAX_CELL_LINES: usize = 4096;
/// Top of the running-header text band (within the top margin).
const HEADER_Y: f32 = 24.0;
/// Gap below the content box before the running-footer band.
const FOOTER_GAP: f32 = 8.0;
/// Hard cap on grid columns / a cell's column or row span, so a hostile model
/// (e.g. `col_span = u16::MAX`) cannot amplify into millions of cells/elements.
/// Far above any real document (Excel maxes at 16384 columns).
const MAX_TABLE_COLS: usize = 1024;
const EMU_PER_PT: f32 = 12_700.0;
const MAX_FLOATING_SHAPE_OVERLAYS: usize = 64;
const SMALL_CAPS_SCALE: f32 = 0.8;
const VERTICAL_ALIGN_SCALE: f32 = 0.65;
const MAX_CELL_INSET_PT: f32 = 720.0;
const DEFAULT_TAB_STOP_PT: f32 = 36.0;
const COLUMN_GAP_PT: f32 = 18.0;
const MIN_COLUMN_WIDTH_PT: f32 = 20.0;
const MAX_SECTION_COLUMNS: usize = 64;
const RIGHT_TO_LEFT_MARK: char = '\u{200F}';
const RIGHT_TO_LEFT_ISOLATE: char = '\u{2067}';
const POP_DIRECTIONAL_ISOLATE: char = '\u{2069}';

#[derive(Clone, Copy, Default)]
pub(crate) struct SourceRenderHints<'a> {
    pub(crate) pagination: &'a [PaginationHint],
    pub(crate) tab_stops: &'a [Vec<TabStop>],
    pub(crate) default_tab_stop_pt: Option<f32>,
    pub(crate) table_row_pagination: &'a [Vec<TableRowPaginationHint>],
    pub(crate) table_cell_pagination: &'a [TableCellPaginationHints],
    pub(crate) table_nested_pagination: &'a [TableCellNestedPaginationHints],
    pub(crate) table_cell_tab_stops: &'a [TableCellTabStopHints],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DynamicTextKind {
    PageNumber,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DynamicTextRun {
    kind: DynamicTextKind,
    page_field_index: Option<usize>,
    props: CharProps,
}

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
    highlight: Option<rgb::Color>,
    ascent: f32,
    descent: f32,
    baseline_shift: f32,
    underline: Option<TextDecoration>,
    strikethrough: Option<TextDecoration>,
    /// Hyperlink target, if this run is part of a `FieldRole::Hyperlink` range.
    link: Option<Rc<str>>,
    /// Dynamic text to re-shape when the final page context is known.
    dynamic: Option<DynamicTextRun>,
    text: Rc<str>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TextDecoration {
    offset: f32,
    thickness: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct RunPaint {
    color: rgb::Color,
    highlight: Option<rgb::Color>,
    baseline_shift: f32,
    underline: bool,
    strikethrough: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LineBackground {
    color: rgb::Color,
    width: f32,
}

impl RunDraw {
    /// Advance width of the run in points (sum of glyph advances × size).
    fn width(&self) -> f32 {
        self.glyphs.iter().map(|g| g.x_advance).sum::<f32>() * self.size
    }
}

/// A laid-out line: its advance height, the baseline offset from the line top,
/// its left indent (0 inside table cells; set for indented/list paragraphs), and
/// its runs.
#[derive(Clone)]
struct LineLayout {
    height: f32,
    baseline: f32,
    x_indent: f32,
    char_range: Option<LineCharRange>,
    background: Option<LineBackground>,
    cell_spacing: CellLineSpacing,
    cell_paragraph: Option<CellParagraphLine>,
    cell_cant_split_group: Option<NonZeroUsize>,
    runs: Vec<RunDraw>,
}

impl LineLayout {
    fn cell_extent(&self) -> f32 {
        self.cell_spacing.before + self.height + self.cell_spacing.after
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct CellLineSpacing {
    before: f32,
    after: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellParagraphLine {
    scope_id: usize,
    paragraph_id: usize,
    line_index: usize,
    line_count: usize,
    pagination: PaginationHint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineCharRange {
    start: usize,
    end: usize,
}

impl LineCharRange {
    fn contains(self, offset: usize) -> bool {
        self.start <= offset && offset <= self.end
    }
}

/// A bordered table cell: its canonical horizontal edges and modeled width
/// (relative to the content origin), its wrapped rich text lines, the background
/// fill, and the vertical alignment.
#[derive(Clone)]
struct CellBox {
    x: f32,
    right: f32,
    width: f32,
    lines: Vec<LineLayout>,
    insets: CellInsets,
    shading: Option<rgb::Color>,
    valign: VCell,
    border_edges: CellBorderEdges,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CellInsets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl CellInsets {
    fn zero() -> Self {
        Self {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        }
    }
}

/// One table row: its height and the cells across it (including empty cells where
/// a `row_span` from an earlier row covers a column).
#[derive(Clone)]
struct RowLayout {
    height: f32,
    cells: Vec<CellBox>,
    cant_split: bool,
    border: TableBorderPaints,
    table_id: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TableBorderPaint {
    color: rgb::Color,
    width: f32,
}

impl Default for TableBorderPaint {
    fn default() -> Self {
        Self {
            color: rgb::Color::black(),
            width: BORDER,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TableBorderPaints {
    top: TableBorderPaint,
    left: TableBorderPaint,
    bottom: TableBorderPaint,
    right: TableBorderPaint,
    inside_h: TableBorderPaint,
    inside_v: TableBorderPaint,
}

impl TableBorderPaints {
    fn get(self, side: TableBorderSide) -> TableBorderPaint {
        match side {
            TableBorderSide::Top => self.top,
            TableBorderSide::Left => self.left,
            TableBorderSide::Bottom => self.bottom,
            TableBorderSide::Right => self.right,
            TableBorderSide::InsideHorizontal => self.inside_h,
            TableBorderSide::InsideVertical => self.inside_v,
        }
    }

    fn with_max_width(self, max_width: f32) -> Self {
        let bound = |paint: TableBorderPaint| TableBorderPaint {
            width: paint.width.min(max_width),
            ..paint
        };
        Self {
            top: bound(self.top),
            left: bound(self.left),
            bottom: bound(self.bottom),
            right: bound(self.right),
            inside_h: bound(self.inside_h),
            inside_v: bound(self.inside_v),
        }
    }
}

impl Default for TableBorderPaints {
    fn default() -> Self {
        let paint = TableBorderPaint::default();
        Self {
            top: paint,
            left: paint,
            bottom: paint,
            right: paint,
            inside_h: paint,
            inside_v: paint,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellBorderEdges {
    top: Option<TableBorderSide>,
    left: Option<TableBorderSide>,
    bottom: Option<TableBorderSide>,
    right: Option<TableBorderSide>,
}

impl CellBorderEdges {
    #[cfg(test)]
    fn outer() -> Self {
        Self {
            top: Some(TableBorderSide::Top),
            left: Some(TableBorderSide::Left),
            bottom: Some(TableBorderSide::Bottom),
            right: Some(TableBorderSide::Right),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CellBorderPaints {
    top: Option<TableBorderPaint>,
    left: Option<TableBorderPaint>,
    bottom: Option<TableBorderPaint>,
    right: Option<TableBorderPaint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct VerticalBorderLine {
    x: f32,
    paint: TableBorderPaint,
}

struct RenderedRowBorders {
    table_id: usize,
    bottom: f32,
    vertical: Vec<VerticalBorderLine>,
}

impl CellBorderPaints {
    fn resolve(edges: CellBorderEdges, paints: TableBorderPaints) -> Self {
        Self {
            top: edges.top.map(|side| paints.get(side)),
            left: edges.left.map(|side| paints.get(side)),
            bottom: edges.bottom.map(|side| paints.get(side)),
            right: edges.right.map(|side| paints.get(side)),
        }
    }

    #[cfg(test)]
    fn uniform(paint: TableBorderPaint) -> Self {
        Self {
            top: Some(paint),
            left: Some(paint),
            bottom: Some(paint),
            right: Some(paint),
        }
    }
}

#[derive(Clone, Copy)]
struct TopBottomBand {
    top: f32,
    bottom: f32,
    anchor_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ImageLayout {
    image_w: f32,
    image_h: f32,
    bounds_w: f32,
    bounds_h: f32,
    rotation_degrees: i32,
}

/// A unit of block flow, paginated top-to-bottom. `Table` groups its rows (with the
/// header-row count) so pagination can repeat headers and split oversized rows;
/// `Row` is an individual placed row produced during pagination.
enum FlowItem {
    BlockStart {
        index: usize,
        pagination: PaginationHint,
    },
    TopBottomBand {
        top: f32,
        bottom: f32,
        anchor_offset: usize,
    },
    PaginationBoundary,
    Gap(f32),
    Line(LineLayout),
    Row(RowLayout),
    PageBreak,
    SectionBreak(SectionSetup),
    Table {
        rows: Vec<RowLayout>,
        header_rows: usize,
    },
    Picture {
        image: PdfImage,
        layout: ImageLayout,
    },
    Chart {
        chart: Chart,
        w: f32,
        h: f32,
    },
}

#[derive(Default)]
struct LayoutCapture {
    collect_page_fields: bool,
    page_fields: Vec<Option<usize>>,
    next_table_id: usize,
}

impl LayoutCapture {
    fn page_fields() -> Self {
        Self {
            collect_page_fields: true,
            page_fields: Vec::new(),
            next_table_id: 0,
        }
    }

    fn register_page_field(&mut self) -> Option<usize> {
        if !self.collect_page_fields {
            return None;
        }
        let index = self.page_fields.len();
        self.page_fields.push(None);
        Some(index)
    }

    fn allocate_table_id(&mut self) -> usize {
        let id = self.next_table_id;
        self.next_table_id = self.next_table_id.saturating_add(1);
        id
    }
}

#[derive(Clone)]
struct RenderPageSection {
    setup: SectionSetup,
    first_page_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct FloatingShapeOverlay {
    page_index: usize,
    behind_doc: bool,
    label: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BlockLinePage {
    page_index: usize,
    range: LineCharRange,
}

#[derive(Debug, Clone, Copy)]
enum ShapeAxis {
    Horizontal,
    Vertical,
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

fn decode_model_image(img: &Image) -> Option<(PdfImage, u32, u32)> {
    let bytes = img.bytes.as_ref()?;
    if img.mime.as_deref() == Some(crate::image::MIME_RAW_RGBA) {
        let (width, height) = (img.width_px?, img.height_px?);
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if bytes.len() != expected {
            return None;
        }
        return Some((
            PdfImage::from_rgba8(bytes.clone(), width, height),
            width,
            height,
        ));
    }
    decode_image(bytes, img.mime.as_deref())
}

fn clockwise_rotation_components(degrees: i32) -> (f32, f32) {
    match degrees.rem_euclid(360) {
        0 => (1.0, 0.0),
        90 => (0.0, 1.0),
        180 => (-1.0, 0.0),
        270 => (0.0, -1.0),
        degrees => {
            let radians = (degrees as f32).to_radians();
            (radians.cos(), radians.sin())
        }
    }
}

fn image_layout(
    width_px: u32,
    height_px: u32,
    rotation_degrees: Option<i32>,
    max_w: f32,
    max_h: f32,
) -> Option<ImageLayout> {
    let mut image_w = width_px as f32 * 0.75;
    let mut image_h = height_px as f32 * 0.75;
    let rotation_degrees = rotation_degrees.unwrap_or(0).rem_euclid(360);
    let (cos, sin) = clockwise_rotation_components(rotation_degrees);
    let mut bounds_w = image_w * cos.abs() + image_h * sin.abs();
    let mut bounds_h = image_w * sin.abs() + image_h * cos.abs();
    if ![image_w, image_h, bounds_w, bounds_h, max_w, max_h]
        .into_iter()
        .all(|value| value.is_finite() && value > 0.0)
    {
        return None;
    }

    if bounds_w > max_w {
        let scale = max_w / bounds_w;
        image_w *= scale;
        image_h *= scale;
        bounds_w = max_w;
        bounds_h *= scale;
    }
    if bounds_h > max_h {
        let scale = max_h / bounds_h;
        image_w *= scale;
        image_h *= scale;
        bounds_w *= scale;
        bounds_h = max_h;
    }
    if ![image_w, image_h, bounds_w, bounds_h]
        .into_iter()
        .all(|value| value.is_finite() && value > 0.0)
    {
        return None;
    }
    Some(ImageLayout {
        image_w,
        image_h,
        bounds_w,
        bounds_h,
        rotation_degrees,
    })
}

fn image_paint_transform(layout: ImageLayout, bounds_x: f32, bounds_y: f32) -> Transform {
    if layout.rotation_degrees == 0 {
        return Transform::from_translate(bounds_x, bounds_y);
    }
    let (cos, sin) = clockwise_rotation_components(layout.rotation_degrees);
    let center_x = bounds_x + layout.bounds_w * 0.5;
    let center_y = bounds_y + layout.bounds_h * 0.5;
    let image_center_x = layout.image_w * 0.5;
    let image_center_y = layout.image_h * 0.5;
    Transform::from_row(
        cos,
        sin,
        -sin,
        cos,
        center_x - cos * image_center_x + sin * image_center_y,
        center_y - sin * image_center_x - cos * image_center_y,
    )
}

/// Decode a model image and size it to a [`FlowItem::Picture`] (96-dpi px → PDF
/// points, rotated bounds fit to the content box and one page, aspect preserved).
/// `None` if there are no bytes or the format is undecodable.
fn image_flow_item(img: &Image, geom: Geom) -> Option<FlowItem> {
    let (image, width_px, height_px) = decode_model_image(img)?;
    let layout = image_layout(
        width_px,
        height_px,
        img.rotation_degrees,
        geom.content_w(),
        geom.bottom() - geom.top(),
    )?;
    Some(FlowItem::Picture { image, layout })
}

fn image_is_undecodable(img: &Image) -> bool {
    img.bytes.is_some() && decode_model_image(img).is_none()
}

fn image_missing_bytes(img: &Image) -> bool {
    img.bytes.is_none()
}

fn count_images_matching(blocks: &[Block], matches: fn(&Image) -> bool) -> usize {
    let mut count = 0;
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                count += paragraph
                    .runs
                    .iter()
                    .filter(|run| !run.props.hidden)
                    .filter_map(|run| run.image.as_ref())
                    .filter(|image| matches(image))
                    .count();
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        count += count_images_matching(&cell.blocks, matches);
                    }
                }
            }
            Block::Image(image) if matches(image) => count += 1,
            Block::Image(_) | Block::Chart(_) | Block::PageBreak | Block::SectionBreak(_) => {}
        }
    }
    count
}

fn count_undecodable_images(blocks: &[Block]) -> usize {
    count_images_matching(blocks, image_is_undecodable)
}

fn count_missing_image_bytes(blocks: &[Block]) -> usize {
    count_images_matching(blocks, image_missing_bytes)
}

fn render_warnings_for_model(
    unsupported: &FeatureInventory,
    model: &DocModel,
) -> Vec<RenderWarning> {
    let mut warnings = report::render_warnings_for(unsupported);
    let missing_image_bytes = count_missing_image_bytes(&model.blocks);
    if missing_image_bytes > 0 {
        warnings.push(RenderWarning::MissingImageBytes {
            count: missing_image_bytes,
        });
    }
    let undecodable_images = count_undecodable_images(&model.blocks);
    if undecodable_images > 0 {
        warnings.push(RenderWarning::UndecodableRasterImages {
            count: undecodable_images,
        });
    }
    warnings
}

/// Size an authored chart block for PDF flow (96-dpi px -> PDF points, fit to
/// the content box and one page). Empty charts are skipped rather than rendered
/// as misleading empty axes.
fn chart_flow_item(chart: &Chart, geom: Geom) -> Option<FlowItem> {
    if chart.categories.is_empty() || chart.series.is_empty() {
        return None;
    }
    let mut w = chart.width_px.unwrap_or(480) as f32 * 0.75;
    let mut h = chart.height_px.unwrap_or(320) as f32 * 0.75;
    let content_w = geom.content_w();
    if w > content_w {
        let s = content_w / w;
        w = content_w;
        h *= s;
    }
    let max_h = geom.bottom() - geom.top();
    if h > max_h {
        let s = max_h / h;
        h = max_h;
        w *= s;
    }
    (w > 0.0 && h > 0.0).then_some(FlowItem::Chart {
        chart: chart.clone(),
        w,
        h,
    })
}

/// The system font stack, with Windows/Noto Korean faces preferred so Hangul
/// shapes even before fontique's automatic per-script fallback kicks in.
fn font_stack() -> FontFamily<'static> {
    FontFamily::List(Cow::Borrowed(&[
        FontFamilyName::Named(Cow::Borrowed("Malgun Gothic")),
        FontFamilyName::Named(Cow::Borrowed("Noto Sans CJK KR")),
        FontFamilyName::Named(Cow::Borrowed("Noto Sans KR")),
        FontFamilyName::Named(Cow::Borrowed("Noto Sans Arabic")),
        FontFamilyName::Named(Cow::Borrowed("Noto Sans Hebrew")),
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
        FontFamilyName::Named(Cow::Borrowed("Noto Sans Arabic")),
        FontFamilyName::Named(Cow::Borrowed("Noto Sans Hebrew")),
        FontFamilyName::Named(Cow::Borrowed("Arial")),
    ]))
}

/// The `CharProps` range covering byte `pos`.
fn props_at(ranges: &[(usize, usize, CharProps)], pos: usize) -> Option<&CharProps> {
    // `ranges` are appended in run order, so they are sorted by start and non-overlapping:
    // binary-search the one covering `pos` instead of scanning from the front per cluster,
    // which made shaping O(clusters × runs) = O(N²) on a paragraph of many tiny runs.
    let i = ranges.partition_point(|(s, _, _)| *s <= pos);
    if i == 0 {
        return None;
    }
    let (_, e, p) = &ranges[i - 1];
    (pos < *e).then_some(p)
}

fn model_color(color: Color) -> rgb::Color {
    rgb::Color::new(color.r, color.g, color.b)
}

#[cfg(test)]
fn color_at(ranges: &[(usize, usize, CharProps)], pos: usize) -> rgb::Color {
    props_at(ranges, pos)
        .and_then(|props| props.color)
        .map(model_color)
        .unwrap_or_else(|| rgb::Color::new(0, 0, 0))
}

fn word_highlight(value: Option<&str>) -> Option<rgb::Color> {
    let value = value?.trim();
    let color = match value.to_ascii_lowercase().as_str() {
        "black" => (0x00, 0x00, 0x00),
        "blue" => (0x00, 0x00, 0xFF),
        "cyan" => (0x00, 0xFF, 0xFF),
        "green" => (0x00, 0xFF, 0x00),
        "magenta" => (0xFF, 0x00, 0xFF),
        "red" => (0xFF, 0x00, 0x00),
        "yellow" => (0xFF, 0xFF, 0x00),
        "white" => (0xFF, 0xFF, 0xFF),
        "darkblue" => (0x00, 0x00, 0x80),
        "darkcyan" => (0x00, 0x80, 0x80),
        "darkgreen" => (0x00, 0x80, 0x00),
        "darkmagenta" => (0x80, 0x00, 0x80),
        "darkred" => (0x80, 0x00, 0x00),
        "darkyellow" => (0x80, 0x80, 0x00),
        "darkgray" | "darkgrey" => (0x80, 0x80, 0x80),
        "lightgray" | "lightgrey" => (0xC0, 0xC0, 0xC0),
        _ => return None,
    };
    Some(rgb::Color::new(color.0, color.1, color.2))
}

fn synthetic_font_scale(props: &CharProps) -> f32 {
    let small_caps = if props.small_caps {
        SMALL_CAPS_SCALE
    } else {
        1.0
    };
    let vertical = if props.vert_align == VertAlign::Baseline {
        1.0
    } else {
        VERTICAL_ALIGN_SCALE
    };
    small_caps * vertical
}

fn paint_at(ranges: &[(usize, usize, CharProps)], pos: usize, font_size: f32) -> RunPaint {
    let Some(props) = props_at(ranges, pos) else {
        return default_run_paint();
    };
    let baseline_shift = match props.vert_align {
        VertAlign::Baseline => 0.0,
        VertAlign::Super => -font_size * 0.55,
        VertAlign::Sub => font_size * 0.25,
    };
    RunPaint {
        color: props
            .color
            .map(model_color)
            .unwrap_or_else(|| rgb::Color::new(0, 0, 0)),
        highlight: word_highlight(props.highlight.as_deref()),
        baseline_shift,
        underline: props.underline,
        strikethrough: props.strike,
    }
}

fn default_run_paint() -> RunPaint {
    RunPaint {
        color: rgb::Color::new(0, 0, 0),
        highlight: None,
        baseline_shift: 0.0,
        underline: false,
        strikethrough: false,
    }
}

/// Apply `w:caps`/`w:smallCaps` to a run's text for rendering — both display
/// uppercased (small-caps is approximated as full caps). Render-only: the stored
/// model text keeps its original case, so `text()`/exporters match the source.
fn cased(props: &CharProps, text: &str) -> String {
    if props.caps || props.small_caps {
        text.to_uppercase()
    } else {
        text.to_string()
    }
}

fn font_mapped_text(props: &CharProps, text: &str) -> String {
    let Some(font) = props.font.as_deref() else {
        return text.to_string();
    };
    let normalized = font
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-' && *ch != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if normalized.contains("wingdings") {
        map_chars(text, wingdings_char)
    } else if normalized == "symbol" || normalized.ends_with("symbol") {
        map_chars(text, symbol_char)
    } else {
        text.to_string()
    }
}

fn display_text(props: &CharProps, text: &str) -> String {
    font_mapped_text(props, &cased(props, text))
}

struct StyledDisplaySegment {
    text: String,
    props: CharProps,
    source_start: usize,
    source_end: usize,
}

fn styled_display_segments(props: &CharProps, text: &str) -> Vec<StyledDisplaySegment> {
    let source_len = text.chars().count();
    if text.is_empty() {
        return Vec::new();
    }
    if props.caps || !props.small_caps {
        let mut shaped_props = props.clone();
        // Casing is materialized into the display string. With both properties
        // set, all-caps wins and authored capitals retain their full size.
        shaped_props.caps = false;
        if props.caps {
            shaped_props.small_caps = false;
        }
        return vec![StyledDisplaySegment {
            text: display_text(props, text),
            props: shaped_props,
            source_start: 0,
            source_end: source_len,
        }];
    }

    let mut segments: Vec<StyledDisplaySegment> = Vec::new();
    for (source_start, ch) in text.chars().enumerate() {
        let synthetic_small_cap = ch.is_lowercase();
        let visible = if synthetic_small_cap {
            ch.to_uppercase().collect::<String>()
        } else {
            ch.to_string()
        };
        let mut shaped_props = props.clone();
        shaped_props.caps = false;
        shaped_props.small_caps = synthetic_small_cap;
        let visible = font_mapped_text(&shaped_props, &visible);
        if let Some(last) = segments.last_mut() {
            if last.props == shaped_props && last.source_end == source_start {
                last.text.push_str(&visible);
                last.source_end += 1;
                continue;
            }
        }
        segments.push(StyledDisplaySegment {
            text: visible,
            props: shaped_props,
            source_start,
            source_end: source_start + 1,
        });
    }
    segments
}

fn append_directional_control(
    text: &mut String,
    ranges: &mut Vec<(usize, usize, CharProps)>,
    source_char_ranges: Option<&mut Vec<(usize, usize)>>,
    props: CharProps,
    control: char,
    source_offset: usize,
) {
    let start = text.len();
    text.push(control);
    ranges.push((start, text.len(), props));
    if let Some(source_char_ranges) = source_char_ranges {
        source_char_ranges.push((source_offset, source_offset));
    }
}

fn has_visible_text(text: &str) -> bool {
    text.chars()
        .any(|ch| !ch.is_whitespace() && !is_injected_directional_control(ch))
}

fn is_injected_directional_control(ch: char) -> bool {
    matches!(
        ch,
        RIGHT_TO_LEFT_MARK | RIGHT_TO_LEFT_ISOLATE | POP_DIRECTIONAL_ISOLATE
    )
}

fn drawable_text_range(text: &str, mut range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    while range.start < range.end {
        let Some(ch) = text[range.clone()].chars().next() else {
            break;
        };
        if !is_injected_directional_control(ch) {
            break;
        }
        range.start += ch.len_utf8();
    }
    while range.start < range.end {
        let Some(ch) = text[range.clone()].chars().next_back() else {
            break;
        };
        if !is_injected_directional_control(ch) {
            break;
        }
        range.end -= ch.len_utf8();
    }
    range
}

fn placeholder_label(count: usize, singular: &str, plural: &str, suffix: &str) -> String {
    let label = if count == 1 { singular } else { plural };
    format!("[rwml preview placeholder: {count} {label} {suffix}]")
}

fn emu_to_pt(value: i64) -> f32 {
    value as f32 / EMU_PER_PT
}

fn format_pt(value: f32) -> String {
    let rounded = value.round();
    if (value - rounded).abs() < 0.05 {
        format!("{}", rounded as i32)
    } else {
        format!("{value:.1}")
    }
}

fn shape_position_label(axis: ShapeAxis, pos: Option<&ShapePosition>) -> String {
    let axis_label = match axis {
        ShapeAxis::Horizontal => "x",
        ShapeAxis::Vertical => "y",
    };
    let Some(pos) = pos else {
        return format!("{axis_label} page");
    };
    let relative_from = pos.relative_from.as_deref().unwrap_or("page");
    if let Some(offset) = pos.offset_emu {
        let sign = if offset < 0 { "-" } else { "+" };
        return format!(
            "{axis_label} {relative_from} {sign} {} pt",
            format_pt(emu_to_pt(offset.saturating_abs()))
        );
    }
    if let Some(align) = pos.align.as_deref() {
        return format!("{axis_label} {relative_from} {align}");
    }
    format!("{axis_label} {relative_from}")
}

fn shape_simple_position_label(axis: ShapeAxis, shape: &FloatingShape) -> Option<String> {
    if shape.simple_position_enabled != Some(true) {
        return None;
    }
    let point = shape.simple_position?;
    let (axis_label, value) = match axis {
        ShapeAxis::Horizontal => ("x", point.x_emu),
        ShapeAxis::Vertical => ("y", point.y_emu),
    };
    Some(format!(
        "{axis_label} simplePos {} pt",
        format_pt(emu_to_pt(value))
    ))
}

fn floating_shape_axis_label(shape: &FloatingShape, axis: ShapeAxis) -> String {
    shape_simple_position_label(axis, shape).unwrap_or_else(|| match axis {
        ShapeAxis::Horizontal => shape_position_label(axis, shape.horizontal_position.as_ref()),
        ShapeAxis::Vertical => shape_position_label(axis, shape.vertical_position.as_ref()),
    })
}

fn floating_shape_name(shape: &FloatingShape, index: usize) -> String {
    for value in [
        shape.name.as_deref(),
        shape.description.as_deref(),
        (!shape.id.is_empty()).then_some(shape.id.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    format!("#{index}")
}

fn compact_shape_text_label(prefix: &str, text: &str) -> Option<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }
    let max_chars = 48;
    let value = if normalized.chars().count() > max_chars {
        let mut truncated = normalized.chars().take(max_chars - 3).collect::<String>();
        truncated.push_str("...");
        truncated
    } else {
        normalized
    };
    Some(format!("{prefix} {value}"))
}

fn shape_color_label(prefix: &str, color: crate::Color) -> String {
    format!("{prefix} #{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn shape_effect_extent_label(effect: crate::ShapeEffectExtent) -> String {
    format!(
        "effect l {} pt, t {} pt, r {} pt, b {} pt",
        format_pt(emu_to_pt(effect.left_emu)),
        format_pt(emu_to_pt(effect.top_emu)),
        format_pt(emu_to_pt(effect.right_emu)),
        format_pt(emu_to_pt(effect.bottom_emu))
    )
}

fn shape_distance_label(prefix: &str, distance: crate::ShapeDistance) -> Option<String> {
    let values = [
        ("t", distance.top_emu),
        ("b", distance.bottom_emu),
        ("l", distance.left_emu),
        ("r", distance.right_emu),
    ]
    .into_iter()
    .filter_map(|(label, value)| {
        value.map(|value| format!("{label} {} pt", format_pt(emu_to_pt(value))))
    })
    .collect::<Vec<_>>();
    (!values.is_empty()).then(|| format!("{prefix} {}", values.join(", ")))
}

fn floating_shape_label(shape: &FloatingShape, index: usize, w: f32, h: f32) -> String {
    let mut layout = vec![
        floating_shape_axis_label(shape, ShapeAxis::Horizontal),
        floating_shape_axis_label(shape, ShapeAxis::Vertical),
    ];
    if let Some(relative_height) = shape.relative_height {
        layout.push(format!("z {relative_height}"));
    }
    if let Some(behind_doc) = shape.behind_doc {
        layout.push(if behind_doc { "behind" } else { "front" }.to_string());
    }
    if let Some(distance_label) = shape_distance_label("anchor dist", shape.distance) {
        layout.push(distance_label);
    }
    if let Some(wrapping) = shape.wrapping.as_ref() {
        layout.push(match wrapping.text.as_deref() {
            Some(text) if !text.trim().is_empty() => {
                format!("wrap {} {}", wrapping.kind, text.trim())
            }
            _ => format!("wrap {}", wrapping.kind),
        });
        if let Some(distance_label) = shape_distance_label("wrap dist", wrapping.distance) {
            if let Some(last) = layout.last_mut() {
                last.push(' ');
                last.push_str(&distance_label);
            }
        }
        if !wrapping.polygon.is_empty() {
            if let Some(last) = layout.last_mut() {
                last.push_str(&format!(" wrap polygon {} pts", wrapping.polygon.len()));
            }
        }
    }
    if let Some(geometry) = shape.preset_geometry.as_deref() {
        let geometry = geometry.trim();
        if !geometry.is_empty() {
            layout.push(format!("geometry {geometry}"));
        }
    }
    if let Some(effect) = shape.effect_extent {
        layout.push(shape_effect_extent_label(effect));
    }
    if let Some(color) = shape.fill_color {
        layout.push(shape_color_label("fill", color));
    }
    if let Some(color) = shape.outline_color {
        layout.push(shape_color_label("outline", color));
    }
    if let Some(anchor_label) = shape
        .anchor_text
        .as_deref()
        .and_then(|text| compact_shape_text_label("anchor", text))
    {
        layout.push(anchor_label);
    }
    if let Some(text_label) = shape
        .text
        .as_deref()
        .and_then(|text| compact_shape_text_label("text", text))
    {
        layout.push(text_label);
    }
    format!(
        "floating shape {index}: {} ({} x {} pt, {})",
        floating_shape_name(shape, index),
        format_pt(w),
        format_pt(h),
        layout.join(", ")
    )
}

fn floating_shape_size(shape: &FloatingShape, geom: Geom) -> (f32, f32) {
    let (mut w, mut h) = shape
        .extent
        .map(|extent| (emu_to_pt(extent.cx_emu), emu_to_pt(extent.cy_emu)))
        .unwrap_or((96.0, 48.0));
    let max_w = (geom.page_w - 8.0).max(24.0);
    let max_h = (geom.page_h - 8.0).max(18.0);
    w = w.clamp(24.0, max_w);
    h = h.clamp(18.0, max_h);
    (w, h)
}

fn shape_reference(axis: ShapeAxis, relative_from: Option<&str>, geom: Geom) -> (f32, f32) {
    let relative_from = relative_from.unwrap_or("page").to_ascii_lowercase();
    match axis {
        ShapeAxis::Horizontal => match relative_from.as_str() {
            "page" => (0.0, geom.page_w),
            "leftmargin" => (0.0, geom.left),
            "rightmargin" => (geom.page_w - geom.right, geom.right),
            "margin" => (geom.left, geom.content_w()),
            _ => (geom.left, geom.content_w()),
        },
        ShapeAxis::Vertical => match relative_from.as_str() {
            "page" => (0.0, geom.page_h),
            "topmargin" => (0.0, geom.top_m),
            "bottommargin" => (geom.page_h - geom.bottom_m, geom.bottom_m),
            "margin" => (geom.top(), (geom.bottom() - geom.top()).max(1.0)),
            _ => (geom.top(), (geom.bottom() - geom.top()).max(1.0)),
        },
    }
}

fn aligned_shape_coordinate(base: f32, span: f32, size: f32, align: Option<&str>) -> f32 {
    match align.unwrap_or("left").to_ascii_lowercase().as_str() {
        "center" | "middle" => base + ((span - size) * 0.5).max(0.0),
        "right" | "bottom" | "outside" => base + (span - size).max(0.0),
        _ => base,
    }
}

fn floating_shape_coordinate(
    pos: Option<&ShapePosition>,
    axis: ShapeAxis,
    geom: Geom,
    size: f32,
) -> f32 {
    let (base, span) = shape_reference(axis, pos.and_then(|p| p.relative_from.as_deref()), geom);
    let raw = match pos {
        Some(pos) => pos
            .offset_emu
            .map(|offset| base + emu_to_pt(offset))
            .unwrap_or_else(|| aligned_shape_coordinate(base, span, size, pos.align.as_deref())),
        None => base,
    };
    let page_span = match axis {
        ShapeAxis::Horizontal => geom.page_w,
        ShapeAxis::Vertical => geom.page_h,
    };
    raw.clamp(0.0, (page_span - size).max(0.0))
}

fn floating_shape_simple_coordinate(
    shape: &FloatingShape,
    axis: ShapeAxis,
    size: f32,
    geom: Geom,
) -> Option<f32> {
    if shape.simple_position_enabled != Some(true) {
        return None;
    }
    let point = shape.simple_position?;
    let raw = match axis {
        ShapeAxis::Horizontal => emu_to_pt(point.x_emu),
        ShapeAxis::Vertical => emu_to_pt(point.y_emu),
    };
    let page_span = match axis {
        ShapeAxis::Horizontal => geom.page_w,
        ShapeAxis::Vertical => geom.page_h,
    };
    Some(raw.clamp(0.0, (page_span - size).max(0.0)))
}

fn bounded_top_bottom_vertical_coordinate(
    shape: &FloatingShape,
    geom: Geom,
    height: f32,
) -> Option<f32> {
    if shape.simple_position_enabled == Some(true) {
        return floating_shape_simple_coordinate(shape, ShapeAxis::Vertical, height, geom);
    }
    let position = shape.vertical_position.as_ref()?;
    let relative_from = position.relative_from.as_deref()?.to_ascii_lowercase();
    if !matches!(
        relative_from.as_str(),
        "page" | "margin" | "topmargin" | "bottommargin"
    ) {
        return None;
    }
    let has_supported_coordinate = position.offset_emu.is_some()
        || matches!(
            position
                .align
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("top" | "center" | "middle" | "bottom")
        );
    has_supported_coordinate.then(|| {
        floating_shape_coordinate(
            shape.vertical_position.as_ref(),
            ShapeAxis::Vertical,
            geom,
            height,
        )
    })
}

fn nonnegative_emu_pt(value: Option<i64>) -> f32 {
    emu_to_pt(value.unwrap_or(0).max(0))
}

fn top_bottom_bands_by_block(
    model: &DocModel,
    shapes: &[FloatingShape],
    geom: Geom,
) -> Vec<Vec<TopBottomBand>> {
    let mut bands = vec![Vec::new(); model.blocks.len()];
    for shape in shapes.iter().take(MAX_FLOATING_SHAPE_OVERLAYS) {
        let Some(wrapping) = shape
            .wrapping
            .as_ref()
            .filter(|wrapping| wrapping.kind.eq_ignore_ascii_case("topAndBottom"))
        else {
            continue;
        };
        if shape.behind_doc == Some(true) {
            continue;
        }
        let Some(block_index) = shape
            .anchor_block_index
            .filter(|&index| matches!(model.blocks.get(index), Some(Block::Paragraph(_))))
        else {
            continue;
        };
        let Some(extent) = shape
            .extent
            .filter(|extent| extent.cx_emu > 0 && extent.cy_emu > 0)
        else {
            continue;
        };
        let Some(anchor_offset) = shape.anchor_char_offset else {
            continue;
        };
        let height = emu_to_pt(extent.cy_emu).min(geom.page_h.max(0.0));
        let Some(y) = bounded_top_bottom_vertical_coordinate(shape, geom, height) else {
            continue;
        };
        let effect_top = nonnegative_emu_pt(shape.effect_extent.map(|effect| effect.top_emu));
        let effect_bottom = nonnegative_emu_pt(shape.effect_extent.map(|effect| effect.bottom_emu));
        let distance_top = nonnegative_emu_pt(wrapping.distance.top_emu.or(shape.distance.top_emu));
        let distance_bottom =
            nonnegative_emu_pt(wrapping.distance.bottom_emu.or(shape.distance.bottom_emu));
        let top = (y - effect_top - distance_top).max(geom.top());
        let bottom = (y + height + effect_bottom + distance_bottom).min(geom.bottom());
        if top < bottom {
            bands[block_index].push(TopBottomBand {
                top,
                bottom,
                anchor_offset,
            });
        }
    }
    bands
}

fn floating_shape_overlays_for_pages(
    shapes: &[FloatingShape],
    geom: Geom,
    block_pages: &HashMap<usize, usize>,
    block_line_pages: &HashMap<usize, Vec<BlockLinePage>>,
) -> Vec<FloatingShapeOverlay> {
    let mut ordered_shapes = shapes
        .iter()
        .take(MAX_FLOATING_SHAPE_OVERLAYS)
        .enumerate()
        .collect::<Vec<_>>();
    ordered_shapes.sort_by_key(|(i, shape)| {
        (
            shape.behind_doc != Some(true),
            shape.relative_height.unwrap_or(0),
            *i,
        )
    });
    ordered_shapes
        .into_iter()
        .map(|(i, shape)| {
            let index = i + 1;
            let (w, h) = floating_shape_size(shape, geom);
            let x = floating_shape_simple_coordinate(shape, ShapeAxis::Horizontal, w, geom)
                .unwrap_or_else(|| {
                    floating_shape_coordinate(
                        shape.horizontal_position.as_ref(),
                        ShapeAxis::Horizontal,
                        geom,
                        w,
                    )
                });
            let y = floating_shape_simple_coordinate(shape, ShapeAxis::Vertical, h, geom)
                .unwrap_or_else(|| {
                    floating_shape_coordinate(
                        shape.vertical_position.as_ref(),
                        ShapeAxis::Vertical,
                        geom,
                        h,
                    )
                });
            let page_index =
                floating_shape_anchor_page(shape, block_pages, block_line_pages).unwrap_or(0);
            FloatingShapeOverlay {
                page_index,
                behind_doc: shape.behind_doc == Some(true),
                label: floating_shape_label(shape, index, w, h),
                x,
                y,
                w,
                h,
            }
        })
        .collect()
}

fn floating_shape_anchor_page(
    shape: &FloatingShape,
    block_pages: &HashMap<usize, usize>,
    block_line_pages: &HashMap<usize, Vec<BlockLinePage>>,
) -> Option<usize> {
    let block_index = shape.anchor_block_index?;
    let block_page = block_pages.get(&block_index).copied();
    let Some(anchor_offset) = shape.anchor_char_offset else {
        return block_page;
    };
    block_line_pages
        .get(&block_index)
        .and_then(|lines| {
            lines
                .iter()
                .find(|line| line.range.contains(anchor_offset))
                .map(|line| line.page_index)
        })
        .or(block_page)
}

fn unsupported_placeholder_texts(features: &FeatureInventory) -> Vec<String> {
    let mut placeholders = Vec::new();
    if features.floating_shapes > 0 {
        placeholders.push(placeholder_label(
            features.floating_shapes,
            "floating shape",
            "floating shapes",
            "preserved but not positioned",
        ));
    }
    if features.charts > 0 {
        placeholders.push(placeholder_label(
            features.charts,
            "chart",
            "charts",
            "preserved but not modeled",
        ));
    }
    if features.ole_objects > 0 {
        placeholders.push(placeholder_label(
            features.ole_objects,
            "OLE object",
            "OLE objects",
            "preserved but not modeled",
        ));
    }
    if features.unsupported_metafiles > 0 {
        placeholders.push(placeholder_label(
            features.unsupported_metafiles,
            "WMF/EMF image",
            "WMF/EMF images",
            "preserved but not rendered",
        ));
    }
    placeholders
}

fn unsupported_placeholder_texts_with_known_shapes(
    features: &FeatureInventory,
    known_floating_shapes: usize,
) -> Vec<String> {
    let mut features = features.clone();
    features.floating_shapes = features
        .floating_shapes
        .saturating_sub(known_floating_shapes);
    unsupported_placeholder_texts(&features)
}

fn undecodable_image_placeholder_texts(count: usize) -> Vec<String> {
    if count == 0 {
        Vec::new()
    } else {
        vec![placeholder_label(
            count,
            "raster image",
            "raster images",
            "skipped because the PDF backend could not decode them",
        )]
    }
}

fn missing_image_placeholder_texts(count: usize) -> Vec<String> {
    if count == 0 {
        Vec::new()
    } else {
        vec![placeholder_label(
            count,
            "image",
            "images",
            "unavailable because their bytes were not extracted",
        )]
    }
}

fn placeholder_blocks(texts: Vec<String>) -> Vec<Block> {
    texts
        .into_iter()
        .map(|text| {
            Block::Paragraph(Paragraph {
                props: ParaProps {
                    spacing: Spacing {
                        before_pt: Some(0.0),
                        after_pt: Some(2.0),
                        ..Spacing::default()
                    },
                    ..ParaProps::default()
                },
                runs: vec![Run {
                    text,
                    props: CharProps {
                        italic: true,
                        color: Some(Color::rgb(90, 90, 90)),
                        size_half_pt: Some(18),
                        ..CharProps::default()
                    },
                    ..Run::default()
                }],
            })
        })
        .collect()
}

fn unsupported_placeholder_blocks(
    features: &FeatureInventory,
    known_floating_shapes: usize,
) -> Vec<Block> {
    placeholder_blocks(unsupported_placeholder_texts_with_known_shapes(
        features,
        known_floating_shapes,
    ))
}

fn missing_image_placeholder_blocks(count: usize) -> Vec<Block> {
    placeholder_blocks(missing_image_placeholder_texts(count))
}

fn undecodable_image_placeholder_blocks(count: usize) -> Vec<Block> {
    placeholder_blocks(undecodable_image_placeholder_texts(count))
}

#[cfg(test)]
fn page_field_text(
    props: &CharProps,
    text: &str,
    field: &FieldRole,
    page_number: Option<usize>,
) -> String {
    match (field, page_number) {
        (FieldRole::Simple { instruction }, Some(page_number))
            if FieldKind::from_instruction(instruction) == FieldKind::Page =>
        {
            display_text(props, &page_number.to_string())
        }
        _ => display_text(props, text),
    }
}

fn is_page_field(field: &FieldRole) -> bool {
    matches!(
        field,
        FieldRole::Simple { instruction }
            if FieldKind::from_instruction(instruction) == FieldKind::Page
    )
}

fn page_field_index_for_field(field: &FieldRole, capture: &mut LayoutCapture) -> Option<usize> {
    if is_page_field(field) {
        capture.register_page_field()
    } else {
        None
    }
}

fn dynamic_text_for_field(
    field: &FieldRole,
    props: &CharProps,
    page_field_index: Option<usize>,
) -> Option<DynamicTextRun> {
    match field {
        FieldRole::Simple { instruction }
            if FieldKind::from_instruction(instruction) == FieldKind::Page =>
        {
            let mut props = props.clone();
            props.caps = false;
            props.small_caps = false;
            Some(DynamicTextRun {
                kind: DynamicTextKind::PageNumber,
                page_field_index,
                props,
            })
        }
        _ => None,
    }
}

fn map_chars(text: &str, map: fn(char) -> Option<char>) -> String {
    let mut changed = false;
    let mapped = text
        .chars()
        .map(|ch| {
            if let Some(mapped) = map(ch) {
                changed = true;
                mapped
            } else {
                ch
            }
        })
        .collect::<String>();
    if changed {
        mapped
    } else {
        text.to_string()
    }
}

fn symbol_char(ch: char) -> Option<char> {
    Some(match ch {
        'A' => 'Α',
        'B' => 'Β',
        'C' => 'Χ',
        'D' => 'Δ',
        'E' => 'Ε',
        'F' => 'Φ',
        'G' => 'Γ',
        'H' => 'Η',
        'I' => 'Ι',
        'K' => 'Κ',
        'L' => 'Λ',
        'M' => 'Μ',
        'N' => 'Ν',
        'O' => 'Ο',
        'P' => 'Π',
        'Q' => 'Θ',
        'R' => 'Ρ',
        'S' => 'Σ',
        'T' => 'Τ',
        'U' => 'Υ',
        'W' => 'Ω',
        'X' => 'Ξ',
        'Y' => 'Ψ',
        'Z' => 'Ζ',
        'a' => 'α',
        'b' => 'β',
        'c' => 'χ',
        'd' => 'δ',
        'e' => 'ε',
        'f' => 'φ',
        'g' => 'γ',
        'h' => 'η',
        'i' => 'ι',
        'k' => 'κ',
        'l' => 'λ',
        'm' => 'μ',
        'n' => 'ν',
        'o' => 'ο',
        'p' => 'π',
        'q' => 'θ',
        'r' => 'ρ',
        's' => 'σ',
        't' => 'τ',
        'u' => 'υ',
        'w' => 'ω',
        'x' => 'ξ',
        'y' => 'ψ',
        'z' => 'ζ',
        '\u{00B7}' => '•',
        '\u{00D3}' => '©',
        _ => return None,
    })
}

fn wingdings_char(ch: char) -> Option<char> {
    Some(match ch {
        'A' => '✌',
        'J' => '☺',
        '\u{00FC}' => '✓',
        '\u{00FB}' => '☑',
        '\u{00FE}' => '☒',
        '\u{00A8}' => '◊',
        '\u{00D8}' => '➢',
        '\u{00E0}' => '➔',
        '\u{00E8}' => '➣',
        'l' => '●',
        'n' => '■',
        'u' => '◆',
        _ => return None,
    })
}

/// The hyperlink URL covering byte `pos`, if any. Like color, a link is not a
/// shaping property, so we look it up per cluster and split draw segments on it.
fn link_at(links: &[(usize, usize, Rc<str>)], pos: usize) -> Option<Rc<str>> {
    // Sorted, non-overlapping (appended in run order) — binary-search rather than scan per
    // cluster, which made shaping O(clusters × link-runs).
    let i = links.partition_point(|(s, _, _)| *s <= pos);
    if i == 0 {
        return None;
    }
    let (_, e, u) = &links[i - 1];
    (pos < *e).then(|| u.clone())
}

fn dynamic_at(ranges: &[(usize, usize, DynamicTextRun)], pos: usize) -> Option<DynamicTextRun> {
    let i = ranges.partition_point(|(s, _, _)| *s <= pos);
    if i == 0 {
        return None;
    }
    let (_, e, dynamic) = &ranges[i - 1];
    (pos < *e).then(|| dynamic.clone())
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

/// The shaping/emit context threaded through layout and drawing: parley's font
/// collection and layout arena plus the parley→krilla font cache. These three
/// always travel together, so they are bundled to keep call signatures small.
struct TextCx<'a> {
    font_cx: &'a mut FontContext,
    layout_cx: &'a mut LayoutContext<rgb::Color>,
    font_cache: &'a mut HashMap<u64, Font>,
}

/// The per-character overlay ranges for a shaped string: color/style ranges,
/// hyperlink ranges, and dynamic (page-number) ranges. They are always built and
/// passed as a set, so they travel together.
#[derive(Clone, Copy)]
struct StyledText<'a> {
    ranges: &'a [(usize, usize, CharProps)],
    links: &'a [(usize, usize, Rc<str>)],
    dynamic_ranges: &'a [(usize, usize, DynamicTextRun)],
}

#[derive(Clone, Copy, Default)]
struct ShapeOptions<'a> {
    line_height: Option<f32>,
    text_indent: f32,
    hanging_indent: bool,
    tab_origin: f32,
    tab_stops: &'a [TabStop],
    default_tab_stop_pt: Option<f32>,
    rtl_tabs: bool,
}

#[derive(Clone, Copy)]
struct ParagraphIndentLayout {
    x_indent: f32,
    wrap_width: f32,
    text_indent: f32,
    hanging_indent: bool,
}

fn paragraph_indent_layout(
    props: &ParaProps,
    available_width: f32,
    extra_left: f32,
) -> ParagraphIndentLayout {
    let left = props.indent.left_pt.unwrap_or(0.0).max(0.0) + extra_left.max(0.0);
    let right = props.indent.right_pt.unwrap_or(0.0).max(0.0);
    let first_line = props
        .indent
        .first_line_pt
        .filter(|value| value.is_finite() && *value != 0.0);
    let hanging = props
        .indent
        .hanging_pt
        .filter(|value| value.is_finite() && *value > 0.0);
    let (x_indent, text_indent, hanging_indent) = if let Some(first_line) = first_line {
        (left, first_line, false)
    } else if let Some(hanging) = hanging {
        ((left - hanging).max(0.0), hanging.min(left), true)
    } else {
        (left, 0.0, false)
    };
    ParagraphIndentLayout {
        x_indent,
        wrap_width: (available_width - x_indent - right).max(20.0),
        text_indent,
        hanging_indent,
    }
}

impl<'a> StyledText<'a> {
    /// A styled string with only character-property ranges (no links or dynamics).
    fn plain(ranges: &'a [(usize, usize, CharProps)]) -> StyledText<'a> {
        StyledText {
            ranges,
            links: &[],
            dynamic_ranges: &[],
        }
    }
}

/// Shape a styled text string into positioned lines at a given wrap `width`.
fn shape(
    text: &str,
    styled: StyledText<'_>,
    heading_level: Option<u8>,
    align: Alignment,
    width: f32,
    cx: &mut TextCx<'_>,
) -> Vec<LineLayout> {
    shape_with_options(
        text,
        styled,
        heading_level,
        align,
        width,
        ShapeOptions::default(),
        cx,
    )
}

fn shape_with_options(
    text: &str,
    styled: StyledText<'_>,
    heading_level: Option<u8>,
    align: Alignment,
    width: f32,
    options: ShapeOptions<'_>,
    cx: &mut TextCx<'_>,
) -> Vec<LineLayout> {
    let StyledText {
        ranges,
        links,
        dynamic_ranges,
    } = styled;
    let base_size = heading_size(heading_level);
    let heading = heading_level.is_some();

    let mut builder = cx.layout_cx.ranged_builder(cx.font_cx, text, 1.0, false);
    builder.push_default(StyleProperty::Brush(rgb::Color::new(0, 0, 0)));
    builder.push_default(StyleProperty::FontFamily(font_stack()));
    builder.push_default(StyleProperty::FontSize(base_size));
    let line_height = options
        .line_height
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.clamp(0.25, 10.0))
        .unwrap_or(1.35);
    builder.push_default(StyleProperty::LineHeight(
        parley::style::LineHeight::FontSizeRelative(line_height),
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
        // Authored size is the base for synthetic small capitals and vertical
        // alignment. Parley then shapes the reduced glyphs at their real advance.
        let authored_size = props
            .size_half_pt
            .filter(|half| *half > 0)
            .map(|half| half as f32 / 2.0);
        let scale = synthetic_font_scale(props);
        if authored_size.is_some() || scale != 1.0 {
            builder.push(
                StyleProperty::FontSize(authored_size.unwrap_or(base_size) * scale),
                *s..*e,
            );
        }
        // Authored font family, tried before the Korean-capable fallbacks.
        if let Some(name) = &props.font {
            if !name.is_empty() {
                builder.push(StyleProperty::FontFamily(named_stack(name)), *s..*e);
            }
        }
    }

    let mut layout = builder.build(text);
    if options.text_indent.is_finite() && options.text_indent != 0.0 {
        layout.set_text_indent(
            options.text_indent.clamp(-width + 1.0, width - 1.0),
            IndentOptions {
                each_line: false,
                hanging: options.hanging_indent,
            },
        );
    }
    let layout_rtl = layout.is_rtl();
    let rtl_tabs = options.rtl_tabs && layout_rtl;
    let adjust_tabs = text.contains('\t')
        && if rtl_tabs {
            matches!(align, Alignment::Right | Alignment::Start)
                && (options.tab_stops.is_empty()
                    || options
                        .tab_stops
                        .iter()
                        .any(|stop| stop.alignment != TabAlignment::Clear))
        } else {
            !layout_rtl
                && (matches!(align, Alignment::Left | Alignment::Start)
                    || options
                        .tab_stops
                        .iter()
                        .any(|stop| stop.alignment != TabAlignment::Clear))
        };

    let text_rc: Rc<str> = Rc::from(text);
    let break_width = width.max(1.0);
    // Tab advances resolve only after breaking, so a tab that pushes content
    // past the paragraph box needs its line re-broken with room reserved for
    // the widening. Reservations only ever tighten, so this settles.
    let mut line_caps: Vec<f32> = Vec::new();
    let mut out;
    let mut pass = 0usize;
    loop {
        if line_caps.is_empty() {
            layout.break_all_lines(Some(break_width));
        } else {
            break_lines_with_caps(&mut layout, break_width, &line_caps);
        }
        layout.align(align, Default::default());
        out = shape_extract_lines(&layout, text, &text_rc, ranges, links, dynamic_ranges, cx);
        if !adjust_tabs {
            break;
        }
        let reservations = if rtl_tabs {
            apply_rtl_tab_stops(
                text,
                &mut out,
                options.tab_stops,
                width,
                options.tab_origin,
                options.default_tab_stop_pt,
            )
        } else {
            apply_tab_stops(
                text,
                &mut out,
                options.tab_stops,
                width,
                options.tab_origin,
                options.default_tab_stop_pt,
                matches!(align, Alignment::Left | Alignment::Start),
            )
        };
        pass += 1;
        if pass > TAB_REFLOW_PASSES
            || !tighten_line_caps(&mut line_caps, &reservations, break_width)
        {
            break;
        }
    }
    out
}

/// Build the drawable lines for an already-broken layout.
fn shape_extract_lines(
    layout: &Layout<rgb::Color>,
    text: &str,
    text_rc: &Rc<str>,
    ranges: &[(usize, usize, CharProps)],
    links: &[(usize, usize, Rc<str>)],
    dynamic_ranges: &[(usize, usize, DynamicTextRun)],
    cx: &mut TextCx<'_>,
) -> Vec<LineLayout> {
    let mut out = Vec::new();
    for line in layout.lines() {
        let m = line.metrics();
        let mut baseline = m.ascent + m.leading * 0.5;
        let mut height = m.line_height;
        let mut line_start_byte = usize::MAX;
        let mut line_end_byte = 0usize;
        for run in line.runs() {
            for cluster in run.visual_clusters() {
                let range = cluster.text_range();
                line_start_byte = line_start_byte.min(range.start);
                line_end_byte = line_end_byte.max(range.end);
            }
        }
        let char_range = (line_start_byte != usize::MAX).then(|| LineCharRange {
            start: text[..line_start_byte].chars().count(),
            end: text[..line_end_byte].chars().count(),
        });
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
            let krilla_font = match cx.font_cache.get(&id) {
                Some(f) => f.clone(),
                None => match Font::new(font_data.into(), font.index) {
                    Some(f) => {
                        cx.font_cache.insert(id, f.clone());
                        f
                    }
                    None => continue,
                },
            };
            let font_size = run.font_size();
            let metrics = *run.metrics();
            let mut glyphs: Vec<KrillaGlyph> = Vec::new();
            // Paint and hyperlink can change within a single uniformly-shaped
            // Parley run, so accumulate glyphs into segments and flush each change.
            let mut seg_paint: Option<RunPaint> = None;
            let mut seg_link: Option<Rc<str>> = None;
            let mut seg_dynamic: Option<DynamicTextRun> = None;
            let mut seg_x = run_x;
            for cluster in run.visual_clusters() {
                if cluster.is_ligature_continuation() {
                    let range = drawable_text_range(text, cluster.text_range());
                    if let Some(g) = glyphs.last_mut() {
                        g.text_range.end = g.text_range.end.max(range.end);
                    }
                    continue;
                }
                let paint = paint_at(ranges, cluster.text_range().start, font_size);
                let lk = link_at(links, cluster.text_range().start);
                let dynamic = dynamic_at(dynamic_ranges, cluster.text_range().start);
                if seg_paint.is_some()
                    && (seg_paint != Some(paint) || lk != seg_link || dynamic != seg_dynamic)
                    && !glyphs.is_empty()
                {
                    let previous = seg_paint.unwrap_or_else(default_run_paint);
                    runs.push(RunDraw {
                        x: seg_x,
                        glyphs: std::mem::take(&mut glyphs),
                        font: krilla_font.clone(),
                        size: font_size,
                        color: previous.color,
                        highlight: previous.highlight,
                        ascent: metrics.ascent,
                        descent: metrics.descent,
                        baseline_shift: previous.baseline_shift,
                        underline: previous.underline.then_some(TextDecoration {
                            offset: metrics.underline_offset,
                            thickness: metrics.underline_size.max(0.25),
                        }),
                        strikethrough: previous.strikethrough.then_some(TextDecoration {
                            offset: metrics.strikethrough_offset,
                            thickness: metrics.strikethrough_size.max(0.25),
                        }),
                        link: seg_link.clone(),
                        dynamic: seg_dynamic.clone(),
                        text: text_rc.clone(),
                    });
                    seg_x = x_cursor;
                }
                seg_paint = Some(paint);
                seg_link = lk;
                seg_dynamic = dynamic;
                let text_range = drawable_text_range(text, cluster.text_range());
                for glyph in cluster.glyphs() {
                    if !text_range.is_empty() {
                        glyphs.push(KrillaGlyph::new(
                            GlyphId::new(glyph.id),
                            glyph.advance / font_size,
                            glyph.x / font_size,
                            glyph.y / font_size,
                            0.0,
                            text_range.clone(),
                            None,
                        ));
                    }
                    x_cursor += glyph.advance;
                }
            }
            if !glyphs.is_empty() {
                let paint = seg_paint.unwrap_or_else(default_run_paint);
                runs.push(RunDraw {
                    x: seg_x,
                    glyphs,
                    font: krilla_font,
                    size: font_size,
                    color: paint.color,
                    highlight: paint.highlight,
                    ascent: metrics.ascent,
                    descent: metrics.descent,
                    baseline_shift: paint.baseline_shift,
                    underline: paint.underline.then_some(TextDecoration {
                        offset: metrics.underline_offset,
                        thickness: metrics.underline_size.max(0.25),
                    }),
                    strikethrough: paint.strikethrough.then_some(TextDecoration {
                        offset: metrics.strikethrough_offset,
                        thickness: metrics.strikethrough_size.max(0.25),
                    }),
                    link: seg_link,
                    dynamic: seg_dynamic,
                    text: text_rc.clone(),
                });
            }
        }
        let mut top = -baseline;
        let mut bottom = height - baseline;
        for run in runs.iter().filter(|run| run.baseline_shift != 0.0) {
            top = top.min(run.baseline_shift - run.ascent);
            bottom = bottom.max(run.baseline_shift + run.descent);
        }
        baseline = -top;
        height = bottom - top;
        out.push(LineLayout {
            height,
            baseline,
            x_indent: 0.0,
            char_range,
            background: None,
            cell_spacing: CellLineSpacing::default(),
            cell_paragraph: None,
            cell_cant_split_group: None,
            runs,
        });
    }
    out
}

/// How far a line's tab advances widened it, and how far past the paragraph
/// box the line ended up as a result.
#[derive(Clone, Copy, Default)]
struct TabReservation {
    shift: f32,
    overflow: f32,
}

/// Re-breaking passes allowed while reserving room for tab advances. Caps only
/// tighten, so a small bound keeps the result deterministic.
const TAB_REFLOW_PASSES: usize = 3;

/// Tighten the per-line breaking caps for lines whose tab advances pushed
/// content past the box. Returns whether any cap actually tightened.
fn tighten_line_caps(caps: &mut Vec<f32>, reservations: &[TabReservation], width: f32) -> bool {
    let mut changed = false;
    for (index, reservation) in reservations.iter().enumerate() {
        if reservation.overflow <= 0.5 || !reservation.shift.is_finite() || reservation.shift <= 0.0
        {
            continue;
        }
        let want = (width - reservation.shift).max(1.0);
        if caps.len() <= index {
            caps.resize(index + 1, width);
        }
        if want < caps[index] - 0.001 {
            caps[index] = want;
            changed = true;
        }
    }
    changed
}

/// Break a layout with a per-line maximum advance, so lines that must reserve
/// room for tab advances fit less content.
fn break_lines_with_caps(layout: &mut Layout<rgb::Color>, width: f32, caps: &[f32]) {
    let mut breaker = layout.break_lines();
    breaker.state_mut().set_layout_max_advance(width);
    let mut index = 0usize;
    loop {
        let cap = caps.get(index).copied().unwrap_or(width).clamp(1.0, width);
        breaker.state_mut().set_line_max_advance(cap);
        if breaker.break_next().is_none() {
            break;
        }
        index += 1;
    }
    breaker.finish();
}

#[derive(Clone, Copy, Default)]
struct TabFieldMetrics {
    advance: f32,
    decimal_offset: Option<f32>,
}

fn glyph_text<'a>(text: &'a str, glyph: &KrillaGlyph) -> Option<&'a str> {
    text.get(glyph.text_range.clone())
}

fn tab_field_metrics(
    text: &str,
    line: &LineLayout,
    tab_run_index: usize,
    tab_glyph_index: usize,
) -> TabFieldMetrics {
    let mut metrics = TabFieldMetrics::default();
    let mut found_preferred_decimal = false;
    for (run_index, run) in line.runs.iter().enumerate().skip(tab_run_index) {
        let glyph_start = if run_index == tab_run_index {
            tab_glyph_index.saturating_add(1)
        } else {
            0
        };
        for glyph in run.glyphs.iter().skip(glyph_start) {
            let Some(glyph_text) = glyph_text(text, glyph) else {
                continue;
            };
            if glyph_text == "\t" {
                return metrics;
            }
            let contains_preferred_decimal =
                glyph_text.chars().any(|ch| matches!(ch, '.' | '\u{066B}'));
            let contains_fallback_decimal = glyph_text.contains(',');
            if contains_preferred_decimal && !found_preferred_decimal {
                metrics.decimal_offset =
                    Some((metrics.advance + glyph.x_offset * run.size).max(metrics.advance));
                found_preferred_decimal = true;
            } else if contains_fallback_decimal
                && !found_preferred_decimal
                && metrics.decimal_offset.is_none()
            {
                metrics.decimal_offset =
                    Some((metrics.advance + glyph.x_offset * run.size).max(metrics.advance));
            }
            let advance = glyph.x_advance * run.size;
            if advance.is_finite() {
                metrics.advance += advance.max(0.0);
            }
        }
    }
    metrics
}

fn explicit_tab_field_start(
    tab_stops: &[TabStop],
    cursor: f32,
    field: TabFieldMetrics,
    width: f32,
    origin: f32,
) -> Option<f32> {
    let absolute_cursor = origin + cursor;
    let absolute_end = origin + width;
    if !absolute_cursor.is_finite() || !absolute_end.is_finite() {
        return None;
    }
    tab_stops
        .iter()
        .filter_map(|stop| {
            let alignment_offset = match stop.alignment {
                TabAlignment::Left => 0.0,
                TabAlignment::Center => field.advance / 2.0,
                TabAlignment::Right => field.advance,
                TabAlignment::Decimal => field.decimal_offset.unwrap_or(field.advance),
                TabAlignment::Clear => return None,
            };
            let absolute_field_start = stop.position_pt - alignment_offset;
            let absolute_field_end = absolute_field_start + field.advance;
            (stop.position_pt.is_finite()
                && stop.position_pt > absolute_cursor + f32::EPSILON
                && absolute_field_start >= absolute_cursor
                && absolute_field_end <= absolute_end)
                .then_some((stop.position_pt, absolute_field_start - origin))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, field_start)| field_start)
}

#[cfg(test)]
fn default_tab_field_start(cursor: f32, width: f32, origin: f32) -> f32 {
    default_tab_field_start_with_interval(cursor, width, origin, None)
}

fn default_tab_field_start_with_interval(
    cursor: f32,
    width: f32,
    origin: f32,
    default_tab_stop_pt: Option<f32>,
) -> f32 {
    let absolute_cursor = origin + cursor;
    let absolute_end = origin + width;
    if !absolute_cursor.is_finite() || !absolute_end.is_finite() {
        return cursor.max(0.0).min(width);
    }
    if absolute_cursor >= absolute_end {
        return width;
    }
    let interval = default_tab_stop_pt
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_TAB_STOP_PT);
    let absolute_stop = (((absolute_cursor / interval).floor() + 1.0) * interval)
        .min(absolute_end)
        .max(absolute_cursor);
    (absolute_stop - origin).max(cursor).min(width)
}

fn apply_tab_stops(
    text: &str,
    lines: &mut [LineLayout],
    tab_stops: &[TabStop],
    width: f32,
    origin: f32,
    default_tab_stop_pt: Option<f32>,
    use_default_fallback: bool,
) -> Vec<TabReservation> {
    let mut reservations = Vec::with_capacity(lines.len());
    for line in lines {
        let mut accumulated_shift = 0.0;
        let mut line_end: f32 = 0.0;
        for run_index in 0..line.runs.len() {
            line.runs[run_index].x += accumulated_shift;
            let mut cursor = line.runs[run_index].x;
            for glyph_index in 0..line.runs[run_index].glyphs.len() {
                let run_size = line.runs[run_index].size;
                let glyph = &line.runs[run_index].glyphs[glyph_index];
                let original_advance = glyph.x_advance * run_size;
                if glyph_text(text, glyph) == Some("\t") && run_size > 0.0 {
                    let field = tab_field_metrics(text, line, run_index, glyph_index);
                    let field_start = explicit_tab_field_start(
                        tab_stops, cursor, field, width, origin,
                    )
                    .or_else(|| {
                        use_default_fallback.then(|| {
                            default_tab_field_start_with_interval(
                                cursor,
                                width,
                                origin,
                                default_tab_stop_pt,
                            )
                        })
                    });
                    if let Some(field_start) = field_start {
                        let advance = (field_start - cursor)
                            .max(0.0)
                            .min((width - cursor).max(0.0));
                        line.runs[run_index].glyphs[glyph_index].x_advance = advance / run_size;
                        accumulated_shift += advance - original_advance;
                        cursor += advance;
                    } else {
                        cursor += original_advance;
                    }
                } else {
                    cursor += original_advance;
                }
            }
            line_end = line_end.max(cursor);
        }
        reservations.push(TabReservation {
            shift: accumulated_shift,
            overflow: (line_end - width).max(0.0),
        });
    }
    reservations
}

fn rtl_tab_field_metrics(
    text: &str,
    line: &LineLayout,
    tab_run_index: usize,
    tab_glyph_index: usize,
) -> TabFieldMetrics {
    let mut metrics = TabFieldMetrics::default();
    let mut found_preferred_decimal = false;
    for run_index in (0..=tab_run_index).rev() {
        let glyph_end = if run_index == tab_run_index {
            tab_glyph_index
        } else {
            line.runs[run_index].glyphs.len()
        };
        for glyph in line.runs[run_index].glyphs[..glyph_end].iter().rev() {
            let Some(glyph_text) = glyph_text(text, glyph) else {
                continue;
            };
            if glyph_text == "\t" {
                return metrics;
            }
            let run_size = line.runs[run_index].size;
            let contains_preferred_decimal =
                glyph_text.chars().any(|ch| matches!(ch, '.' | '\u{066B}'));
            let contains_fallback_decimal = glyph_text.contains(',');
            if contains_preferred_decimal && !found_preferred_decimal {
                metrics.decimal_offset =
                    Some((metrics.advance + glyph.x_offset * run_size).max(metrics.advance));
                found_preferred_decimal = true;
            } else if contains_fallback_decimal
                && !found_preferred_decimal
                && metrics.decimal_offset.is_none()
            {
                metrics.decimal_offset =
                    Some((metrics.advance + glyph.x_offset * run_size).max(metrics.advance));
            }
            let glyph_advance = glyph.x_advance * run_size;
            if glyph_advance.is_finite() {
                metrics.advance += glyph_advance.max(0.0);
            }
        }
    }
    metrics
}

fn explicit_rtl_tab_field_start(
    tab_stops: &[TabStop],
    cursor: f32,
    field: TabFieldMetrics,
    width: f32,
    origin: f32,
) -> Option<f32> {
    let absolute_cursor = origin + cursor;
    let absolute_end = origin + width;
    if !absolute_cursor.is_finite() || !absolute_end.is_finite() {
        return None;
    }
    tab_stops
        .iter()
        .filter_map(|stop| {
            let alignment_offset = match stop.alignment {
                TabAlignment::Left => 0.0,
                TabAlignment::Center => field.advance / 2.0,
                TabAlignment::Right => field.advance,
                TabAlignment::Decimal => field.decimal_offset.unwrap_or(field.advance),
                TabAlignment::Clear => return None,
            };
            let absolute_field_start = stop.position_pt - alignment_offset;
            let absolute_field_end = absolute_field_start + field.advance;
            (stop.position_pt.is_finite()
                && stop.position_pt > absolute_cursor + f32::EPSILON
                && absolute_field_start >= absolute_cursor
                && absolute_field_end <= absolute_end)
                .then_some((stop.position_pt, absolute_field_start - origin))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, field_start)| field_start)
}

fn apply_rtl_tab_stops(
    text: &str,
    lines: &mut [LineLayout],
    tab_stops: &[TabStop],
    width: f32,
    origin: f32,
    default_tab_stop_pt: Option<f32>,
) -> Vec<TabReservation> {
    let use_default_fallback = tab_stops.is_empty()
        || tab_stops
            .iter()
            .all(|stop| matches!(stop.alignment, TabAlignment::Left | TabAlignment::Clear));
    let mut reservations = Vec::with_capacity(lines.len());
    for line in lines {
        let mut run_deltas = vec![0.0_f32; line.runs.len()];
        let mut shift_from_right = 0.0_f32;
        for run_index in (0..line.runs.len()).rev() {
            for glyph_index in (0..line.runs[run_index].glyphs.len()).rev() {
                let run_size = line.runs[run_index].size;
                if !run_size.is_finite()
                    || run_size <= 0.0
                    || glyph_text(text, &line.runs[run_index].glyphs[glyph_index]) != Some("\t")
                {
                    continue;
                }
                let tab_end = line.runs[run_index].x
                    + line.runs[run_index]
                        .glyphs
                        .iter()
                        .take(glyph_index + 1)
                        .map(|glyph| glyph.x_advance * run_size)
                        .sum::<f32>();
                if !tab_end.is_finite() || !shift_from_right.is_finite() {
                    continue;
                }
                let cursor = width - tab_end + shift_from_right;
                // ISO/IEC 29500-1 17.3.1.37 measures w:pos from the paragraph's
                // leading edge, which is the right edge for an RTL paragraph.
                let field = rtl_tab_field_metrics(text, line, run_index, glyph_index);
                let field_start = explicit_rtl_tab_field_start(
                    tab_stops, cursor, field, width, origin,
                )
                .or_else(|| {
                    use_default_fallback.then(|| {
                        default_tab_field_start_with_interval(
                            cursor,
                            width,
                            origin,
                            default_tab_stop_pt,
                        )
                    })
                });
                let Some(field_start) = field_start else {
                    continue;
                };
                let original_advance =
                    line.runs[run_index].glyphs[glyph_index].x_advance * run_size;
                let advance = (field_start - cursor)
                    .max(0.0)
                    .min((width - cursor).max(0.0));
                let delta = advance - original_advance;
                line.runs[run_index].glyphs[glyph_index].x_advance = advance / run_size;
                run_deltas[run_index] += delta;
                shift_from_right += delta;
            }
        }

        let mut run_shift = 0.0_f32;
        for run_index in (0..line.runs.len()).rev() {
            run_shift += run_deltas[run_index];
            line.runs[run_index].x -= run_shift;
        }
        let line_start = line.runs.iter().map(|run| run.x).fold(width, f32::min);
        reservations.push(TabReservation {
            shift: shift_from_right,
            overflow: (-line_start).max(0.0),
        });
    }
    reservations
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

fn paragraph_list_marker(p: &Paragraph, lists: &mut ListState) -> Option<String> {
    match (&p.props.list, p.props.heading_level) {
        (Some(list), None) => Some(lists.marker(list)),
        _ => None,
    }
}

/// Lay out one paragraph into flow items, with an optional list `marker` and the
/// paragraph's left/right indent (list level adds a per-level indent).
struct ShapedParagraph<'a> {
    lines: Vec<LineLayout>,
    images: Vec<&'a Image>,
}

#[allow(clippy::too_many_arguments)]
fn shape_paragraph_content<'a>(
    p: &'a Paragraph,
    marker: Option<&str>,
    tab_stops: &[TabStop],
    default_tab_stop_pt: Option<f32>,
    available_width: f32,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    track_source_ranges: bool,
) -> ShapedParagraph<'a> {
    let list_level = p.props.list.as_ref().map(|l| l.level).unwrap_or(0) as f32;
    let indent = paragraph_indent_layout(&p.props, available_width, list_level * LIST_INDENT);

    let mut text = String::new();
    let mut ranges: Vec<(usize, usize, CharProps)> = Vec::new();
    let mut links: Vec<(usize, usize, Rc<str>)> = Vec::new();
    let mut dynamic_ranges: Vec<(usize, usize, DynamicTextRun)> = Vec::new();
    let mut images: Vec<&Image> = Vec::new();
    let mut source_char_ranges = track_source_ranges.then(Vec::new);
    let mut source_chars = 0usize;
    if p.props.bidi {
        append_directional_control(
            &mut text,
            &mut ranges,
            source_char_ranges.as_mut(),
            CharProps {
                rtl: true,
                ..CharProps::default()
            },
            RIGHT_TO_LEFT_MARK,
            source_chars,
        );
    }
    if let Some(m) = marker {
        if !m.is_empty() {
            let marker_start = text.len();
            text.push_str(m);
            text.push(' ');
            ranges.push((marker_start, text.len(), CharProps::default()));
            if let Some(source_char_ranges) = source_char_ranges.as_mut() {
                source_char_ranges.extend(std::iter::repeat_n(
                    (source_chars, source_chars),
                    m.chars().count() + 1,
                ));
            }
        }
    }
    for r in &p.runs {
        let run_source_chars = source_char_ranges
            .as_ref()
            .map_or(0, |_| r.text.chars().count());
        if r.props.hidden {
            source_chars = source_chars.saturating_add(run_source_chars);
            continue;
        }
        // The reader carries images as inline runs (Run.image); flow them as
        // block pictures after the paragraph's text.
        if let Some(img) = &r.image {
            images.push(img);
        }
        let page_field_index = page_field_index_for_field(&r.field, capture);
        if r.text.is_empty() {
            continue;
        }
        if r.props.rtl {
            append_directional_control(
                &mut text,
                &mut ranges,
                source_char_ranges.as_mut(),
                r.props.clone(),
                RIGHT_TO_LEFT_ISOLATE,
                source_chars,
            );
        }
        let s = text.len();
        for segment in styled_display_segments(&r.props, &r.text) {
            let segment_start = text.len();
            text.push_str(&segment.text);
            ranges.push((segment_start, text.len(), segment.props));
            if let Some(source_char_ranges) = source_char_ranges.as_mut() {
                let rendered_chars = segment.text.chars().count();
                let segment_source_chars = segment.source_end.saturating_sub(segment.source_start);
                for index in 0..rendered_chars {
                    let source_start = index.saturating_mul(segment_source_chars) / rendered_chars;
                    let source_end = (index + 1)
                        .saturating_mul(segment_source_chars)
                        .saturating_add(rendered_chars - 1)
                        / rendered_chars;
                    source_char_ranges.push((
                        source_chars
                            .saturating_add(segment.source_start)
                            .saturating_add(source_start),
                        source_chars
                            .saturating_add(segment.source_start)
                            .saturating_add(source_end.min(segment_source_chars)),
                    ));
                }
            }
        }
        source_chars = source_chars.saturating_add(run_source_chars);
        if let FieldRole::Hyperlink { url } = &r.field {
            links.push((s, text.len(), Rc::from(url.as_str())));
        }
        if let Some(dynamic) = dynamic_text_for_field(&r.field, &r.props, page_field_index) {
            dynamic_ranges.push((s, text.len(), dynamic));
        }
        if r.props.rtl {
            append_directional_control(
                &mut text,
                &mut ranges,
                source_char_ranges.as_mut(),
                r.props.clone(),
                POP_DIRECTIONAL_ISOLATE,
                source_chars,
            );
        }
    }
    let mut lines = Vec::new();
    if has_visible_text(&text) {
        let align = match p.props.align {
            Align::Left => Alignment::Left,
            Align::Center => Alignment::Center,
            Align::Right => Alignment::Right,
            Align::Justify => Alignment::Justify,
        };
        for mut line in shape_with_options(
            &text,
            StyledText {
                ranges: &ranges,
                links: &links,
                dynamic_ranges: &dynamic_ranges,
            },
            p.props.heading_level,
            align,
            indent.wrap_width,
            ShapeOptions {
                line_height: p.props.spacing.line_pct,
                text_indent: indent.text_indent,
                hanging_indent: indent.hanging_indent,
                tab_origin: if p.props.bidi {
                    (available_width - indent.x_indent - indent.wrap_width).max(0.0)
                } else {
                    indent.x_indent
                },
                tab_stops,
                default_tab_stop_pt,
                rtl_tabs: p.props.bidi,
            },
            cx,
        ) {
            line.x_indent = indent.x_indent;
            line.background = p.props.shading.map(|color| LineBackground {
                color: model_color(color),
                width: indent.wrap_width,
            });
            if let Some(source_char_ranges) = source_char_ranges.as_ref() {
                if let Some(range) = line.char_range {
                    line.char_range = (range.start < range.end)
                        .then(|| source_char_ranges.get(range.start..range.end))
                        .flatten()
                        .and_then(|mapped| {
                            mapped
                                .first()
                                .zip(mapped.last())
                                .map(|(first, last)| LineCharRange {
                                    start: first.0,
                                    end: last.1,
                                })
                        });
                }
            } else {
                line.char_range = None;
            }
            lines.push(line);
        }
    }
    ShapedParagraph { lines, images }
}

#[allow(clippy::too_many_arguments)]
fn layout_paragraph(
    p: &Paragraph,
    out: &mut Vec<FlowItem>,
    marker: Option<&str>,
    tab_stops: &[TabStop],
    default_tab_stop_pt: Option<f32>,
    geom: Geom,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) {
    let shaped = shape_paragraph_content(
        p,
        marker,
        tab_stops,
        default_tab_stop_pt,
        geom.content_w(),
        cx,
        capture,
        true,
    );
    out.extend(shaped.lines.into_iter().map(FlowItem::Line));
    for img in shaped.images {
        if let Some(item) = image_flow_item(img, geom) {
            out.push(FlowItem::Gap(PARA_GAP));
            out.push(item);
        }
    }
}

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

fn cell_insets(margins: Option<CellMargins>, width: f32) -> CellInsets {
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
/// Nested tables/images in a cell are not laid out as grids — only their text was
/// ever surfaced. A **nested table** inside a cell is flattened to its cells'
/// lines (no nested grid), recursively — so a document wrapped in an outer table
/// of inner tables still renders its text. Recursion is depth-capped.
#[cfg(test)]
fn shape_cell(
    cell: &Cell,
    inner_w: f32,
    depth: u32,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) -> Vec<LineLayout> {
    shape_cell_with_pagination(cell, None, None, None, None, inner_w, depth, cx, capture)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn shape_cell_with_pagination(
    cell: &Cell,
    pagination: Option<&[Option<PaginationHint>]>,
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
        tab_stops,
        nested_pagination,
        default_tab_stop_pt,
        inner_w,
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
    tab_stops: Option<&[Vec<TabStop>]>,
    nested_pagination: Option<&[Option<TablePaginationHints>]>,
    default_tab_stop_pt: Option<f32>,
    inner_w: f32,
    depth: u32,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    lists: &mut ListState,
) -> Vec<LineLayout> {
    let mut state = CellShapeState {
        next_scope_id: 1,
        next_paragraph_id: 0,
        next_cant_split_group_id: 1,
    };
    shape_cell_in_scope(
        cell,
        pagination,
        tab_stops,
        nested_pagination,
        default_tab_stop_pt,
        inner_w,
        depth,
        cx,
        capture,
        0,
        &mut state,
        lists,
    )
}

struct CellShapeState {
    next_scope_id: usize,
    next_paragraph_id: usize,
    next_cant_split_group_id: usize,
}

fn explicit_cell_spacing(value: Option<f32>) -> f32 {
    value
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(0.0)
}

fn truncate_cell_paragraph_lines(lines: &mut Vec<LineLayout>, remaining: usize, spacing: Spacing) {
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
    fn allocate_scope(&mut self) -> usize {
        let value = self.next_scope_id;
        self.next_scope_id = self.next_scope_id.saturating_add(1);
        value
    }

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
    tab_stops: Option<&[Vec<TabStop>]>,
    nested_pagination: Option<&[Option<TablePaginationHints>]>,
    default_tab_stop_pt: Option<f32>,
    inner_w: f32,
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
                let ShapedParagraph {
                    lines: mut paragraph_lines,
                    ..
                } = shape_paragraph_content(
                    p,
                    marker.as_deref(),
                    paragraph_tab_stops,
                    default_tab_stop_pt,
                    inner_w,
                    cx,
                    capture,
                    false,
                );
                if paragraph_lines.is_empty() {
                    continue;
                }
                let remaining = MAX_CELL_LINES.saturating_sub(lines.len());
                truncate_cell_paragraph_lines(&mut paragraph_lines, remaining, p.props.spacing);
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
                lines.extend(paragraph_lines);
            }
            Block::Table(t) => {
                let table_pagination = nested_pagination
                    .and_then(|tables| tables.get(block_index))
                    .and_then(Option::as_ref);
                for (row_index, row) in t.rows.iter().enumerate() {
                    let row_start = lines.len();
                    for (cell_index, c) in row.cells.iter().enumerate() {
                        let nested_cell_pagination = table_pagination
                            .and_then(|table| table.cells.get(row_index))
                            .and_then(|row| row.get(cell_index))
                            .map(Vec::as_slice);
                        let nested_cell_tab_stops = table_pagination
                            .and_then(|table| table.cell_tabs.get(row_index))
                            .and_then(|row| row.get(cell_index))
                            .map(Vec::as_slice);
                        let nested_cell_tables = table_pagination
                            .and_then(|table| table.nested.get(row_index))
                            .and_then(|row| row.get(cell_index))
                            .map(Vec::as_slice);
                        let nested_scope_id = state.allocate_scope();
                        lines.extend(shape_cell_in_scope(
                            c,
                            nested_cell_pagination,
                            nested_cell_tab_stops,
                            nested_cell_tables,
                            default_tab_stop_pt,
                            inner_w,
                            depth + 1,
                            cx,
                            capture,
                            nested_scope_id,
                            state,
                            lists,
                        ));
                    }
                    let cant_split = table_pagination
                        .and_then(|table| table.rows.get(row_index))
                        .map(|row| row.cant_split)
                        .unwrap_or(false);
                    if cant_split && row_start < lines.len() {
                        if let Some(group_id) = state.allocate_cant_split_group() {
                            for line in &mut lines[row_start..] {
                                line.cell_cant_split_group = Some(group_id);
                            }
                        }
                    }
                }
            }
            Block::Image(_) | Block::Chart(_) | Block::PageBreak | Block::SectionBreak(_) => {}
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
fn layout_table(
    t: &Table,
    out: &mut Vec<FlowItem>,
    geom: Geom,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) {
    layout_table_with_row_pagination(t, out, geom, cx, capture, TablePaginationView::default());
}

#[derive(Clone, Copy, Default)]
struct TablePaginationView<'a> {
    rows: Option<&'a [TableRowPaginationHint]>,
    cells: Option<&'a TableCellPaginationHints>,
    nested: Option<&'a TableCellNestedPaginationHints>,
    cell_tabs: Option<&'a TableCellTabStopHints>,
    default_tab_stop_pt: Option<f32>,
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

fn table_border_paints(t: &Table) -> TableBorderPaints {
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
fn layout_table_with_row_pagination(
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
fn layout_table_with_row_pagination_and_lists(
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
            let (direct_pagination, direct_tab_stops, direct_nested_pagination) =
                if pc.cell.is_some() {
                    let paragraph_hints = row_cell_pagination
                        .and_then(|cells| cells.get(source_cell_index))
                        .map(Vec::as_slice);
                    let paragraph_tab_stops = row_cell_tab_stops
                        .and_then(|cells| cells.get(source_cell_index))
                        .map(Vec::as_slice);
                    let nested_hints = row_nested_pagination
                        .and_then(|cells| cells.get(source_cell_index))
                        .map(Vec::as_slice);
                    source_cell_index += 1;
                    (paragraph_hints, paragraph_tab_stops, nested_hints)
                } else {
                    (None, None, None)
                };
            let (lines, insets, shading, valign) = match pc.cell {
                Some(c) => {
                    let insets = cell_insets(c.margins, width);
                    let lines = shape_cell_with_pagination_and_lists(
                        c,
                        direct_pagination,
                        direct_tab_stops,
                        direct_nested_pagination,
                        pagination.default_tab_stop_pt,
                        (width - insets.left - insets.right).max(1.0),
                        0,
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
fn legal_cell_split(lines: &[LineLayout], cut: usize) -> bool {
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

fn fitting_cell_split(lines: &[LineLayout], budget: f32) -> usize {
    let greedy = greedy_cell_split(lines, budget);
    if greedy >= lines.len() {
        return lines.len();
    }
    (1..=greedy)
        .rev()
        .find(|cut| legal_cell_split(lines, *cut))
        .unwrap_or(greedy)
}

fn split_row(row: RowLayout, avail: f32) -> (RowLayout, Option<RowLayout>) {
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
        let budget = (avail - insets.top - insets.bottom).max(0.0);
        let cut = fitting_cell_split(&lines, budget);
        let mut head = lines;
        let tail = head.split_off(cut);
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

/// Lay out blocks and keep only the text lines (used for running headers/footers,
/// which are drawn compactly in the page margins; tables/images there are rare and
/// dropped).
fn layout_lines(blocks: &[Block], geom: Geom, cx: &mut TextCx<'_>) -> Vec<LineLayout> {
    let mut items = Vec::new();
    let mut capture = LayoutCapture::default();
    collect_blocks(blocks, &mut items, geom, cx, &mut capture);
    items
        .into_iter()
        .filter_map(|i| match i {
            FlowItem::Line(l) => Some(l),
            _ => None,
        })
        .collect()
}

trait RunningSurfaceSetup {
    fn header(&self) -> &[Block];
    fn first_header(&self) -> &[Block];
    fn even_header(&self) -> &[Block];
    fn footer(&self) -> &[Block];
    fn first_footer(&self) -> &[Block];
    fn even_footer(&self) -> &[Block];
    fn title_page(&self) -> bool;
}

impl RunningSurfaceSetup for crate::model::DocSetup {
    fn header(&self) -> &[Block] {
        &self.header
    }

    fn first_header(&self) -> &[Block] {
        &self.first_header
    }

    fn even_header(&self) -> &[Block] {
        &self.even_header
    }

    fn footer(&self) -> &[Block] {
        &self.footer
    }

    fn first_footer(&self) -> &[Block] {
        &self.first_footer
    }

    fn even_footer(&self) -> &[Block] {
        &self.even_footer
    }

    fn title_page(&self) -> bool {
        self.title_page
    }
}

impl RunningSurfaceSetup for SectionSetup {
    fn header(&self) -> &[Block] {
        &self.header
    }

    fn first_header(&self) -> &[Block] {
        &self.first_header
    }

    fn even_header(&self) -> &[Block] {
        &self.even_header
    }

    fn footer(&self) -> &[Block] {
        &self.footer
    }

    fn first_footer(&self) -> &[Block] {
        &self.first_footer
    }

    fn even_footer(&self) -> &[Block] {
        &self.even_footer
    }

    fn title_page(&self) -> bool {
        self.title_page
    }
}

fn running_header_footer_blocks_for_page<T: RunningSurfaceSetup + ?Sized>(
    setup: &T,
    page_number: usize,
    is_first_section_page: bool,
) -> (&[Block], &[Block]) {
    let title_page = is_first_section_page
        && (setup.title_page()
            || !setup.first_header().is_empty()
            || !setup.first_footer().is_empty());
    let header = if title_page {
        setup.first_header()
    } else if page_number % 2 == 0 && !setup.even_header().is_empty() {
        setup.even_header()
    } else {
        setup.header()
    };
    let footer = if title_page {
        setup.first_footer()
    } else if page_number % 2 == 0 && !setup.even_footer().is_empty() {
        setup.even_footer()
    } else {
        setup.footer()
    };
    (header, footer)
}

fn assign_section_to_render_pages(
    page_sections: &mut [Option<RenderPageSection>],
    start_page_index: usize,
    end_page_index: usize,
    setup: &SectionSetup,
) {
    if page_sections.is_empty() {
        return;
    }
    let last_page_index = page_sections.len() - 1;
    let start = start_page_index.min(last_page_index);
    let end = end_page_index.min(last_page_index);
    if start > end {
        return;
    }
    for page_section in &mut page_sections[start..=end] {
        *page_section = Some(RenderPageSection {
            setup: setup.clone(),
            first_page_index: start,
        });
    }
}

fn layout_page_number_line(
    page_number: usize,
    geom: Geom,
    cx: &mut TextCx<'_>,
) -> Option<LineLayout> {
    let text = page_number.to_string();
    shape(
        &text,
        StyledText::plain(&[(0, text.len(), CharProps::default())]),
        None,
        Alignment::Center,
        geom.content_w(),
        cx,
    )
    .into_iter()
    .next()
}

fn collect_blocks(
    blocks: &[Block],
    out: &mut Vec<FlowItem>,
    geom: Geom,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
) {
    collect_blocks_inner(
        blocks,
        out,
        geom,
        cx,
        capture,
        BlockCollectionOptions::default(),
    );
}

#[derive(Default)]
struct BlockCollectionOptions<'a> {
    include_block_anchors: bool,
    section_columns: Option<&'a [Option<u16>]>,
    pagination_hints: Option<&'a [PaginationHint]>,
    tab_stops: Option<&'a [Vec<TabStop>]>,
    default_tab_stop_pt: Option<f32>,
    table_row_pagination: Option<&'a [Vec<TableRowPaginationHint>]>,
    table_cell_pagination: Option<&'a [TableCellPaginationHints]>,
    table_nested_pagination: Option<&'a [TableCellNestedPaginationHints]>,
    table_cell_tab_stops: Option<&'a [TableCellTabStopHints]>,
    top_bottom_bands: Option<&'a [Vec<TopBottomBand>]>,
}

struct BodyCollectionSidecars<'a> {
    section_columns: &'a [Option<u16>],
    pagination_hints: &'a [PaginationHint],
    tab_stops: &'a [Vec<TabStop>],
    default_tab_stop_pt: Option<f32>,
    table_row_pagination: &'a [Vec<TableRowPaginationHint>],
    table_cell_pagination: &'a [TableCellPaginationHints],
    table_nested_pagination: &'a [TableCellNestedPaginationHints],
    table_cell_tab_stops: &'a [TableCellTabStopHints],
    top_bottom_bands: &'a [Vec<TopBottomBand>],
}

fn collect_blocks_with_block_anchors(
    blocks: &[Block],
    out: &mut Vec<FlowItem>,
    geom: Geom,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    sidecars: BodyCollectionSidecars<'_>,
) {
    collect_blocks_inner(
        blocks,
        out,
        geom,
        cx,
        capture,
        BlockCollectionOptions {
            include_block_anchors: true,
            section_columns: Some(sidecars.section_columns),
            pagination_hints: Some(sidecars.pagination_hints),
            tab_stops: Some(sidecars.tab_stops),
            default_tab_stop_pt: sidecars.default_tab_stop_pt,
            table_row_pagination: Some(sidecars.table_row_pagination),
            table_cell_pagination: Some(sidecars.table_cell_pagination),
            table_nested_pagination: Some(sidecars.table_nested_pagination),
            table_cell_tab_stops: Some(sidecars.table_cell_tab_stops),
            top_bottom_bands: Some(sidecars.top_bottom_bands),
        },
    );
}

fn collect_blocks_inner(
    blocks: &[Block],
    out: &mut Vec<FlowItem>,
    geom: Geom,
    cx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    options: BlockCollectionOptions<'_>,
) {
    let mut lists = ListState::default();
    for (block_index, b) in blocks.iter().enumerate() {
        let block_geom = options
            .section_columns
            .and_then(|columns| columns.get(block_index).copied())
            .map(|columns| geom.with_content_width(ColumnLayout::new(geom, columns).width))
            .unwrap_or(geom);
        if options.include_block_anchors {
            out.push(FlowItem::BlockStart {
                index: block_index,
                pagination: options
                    .pagination_hints
                    .and_then(|hints| hints.get(block_index))
                    .copied()
                    .unwrap_or_default(),
            });
        }
        match b {
            Block::Paragraph(p) => {
                if p.props.page_break_before
                    && out
                        .iter()
                        .any(|item| !matches!(item, FlowItem::BlockStart { .. }))
                {
                    out.push(FlowItem::PageBreak);
                }
                if let Some(bands) = options
                    .top_bottom_bands
                    .and_then(|bands| bands.get(block_index))
                {
                    out.extend(bands.iter().map(|band| FlowItem::TopBottomBand {
                        top: band.top,
                        bottom: band.bottom,
                        anchor_offset: band.anchor_offset,
                    }));
                }
                let marker = paragraph_list_marker(p, &mut lists);
                if let Some(before) = p.props.spacing.before_pt.filter(|b| *b > 0.0) {
                    out.push(FlowItem::Gap(before));
                }
                let tab_stops = options
                    .tab_stops
                    .and_then(|stops| stops.get(block_index))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                layout_paragraph(
                    p,
                    out,
                    marker.as_deref(),
                    tab_stops,
                    options.default_tab_stop_pt,
                    block_geom,
                    cx,
                    capture,
                );
                let after = p
                    .props
                    .spacing
                    .after_pt
                    .filter(|value| value.is_finite())
                    .map(|value| value.max(0.0))
                    .unwrap_or(PARA_GAP);
                if after > 0.0 {
                    out.push(FlowItem::Gap(after));
                }
            }
            Block::Table(t) => {
                let row_pagination = options
                    .table_row_pagination
                    .and_then(|tables| tables.get(block_index))
                    .map(Vec::as_slice);
                let cell_pagination = options
                    .table_cell_pagination
                    .and_then(|tables| tables.get(block_index));
                let nested_pagination = options
                    .table_nested_pagination
                    .and_then(|tables| tables.get(block_index));
                let cell_tab_stops = options
                    .table_cell_tab_stops
                    .and_then(|tables| tables.get(block_index));
                layout_table_with_row_pagination_and_lists(
                    t,
                    out,
                    block_geom,
                    cx,
                    capture,
                    TablePaginationView {
                        rows: row_pagination,
                        cells: cell_pagination,
                        nested: nested_pagination,
                        cell_tabs: cell_tab_stops,
                        default_tab_stop_pt: options.default_tab_stop_pt,
                    },
                    &mut lists,
                );
                out.push(FlowItem::Gap(PARA_GAP));
            }
            Block::Image(img) => {
                if let Some(item) = image_flow_item(img, block_geom) {
                    out.push(item);
                    out.push(FlowItem::Gap(PARA_GAP));
                }
            }
            Block::Chart(chart) => {
                if let Some(item) = chart_flow_item(chart, block_geom) {
                    out.push(item);
                    out.push(FlowItem::Gap(PARA_GAP));
                }
            }
            Block::PageBreak => out.push(FlowItem::PageBreak),
            Block::SectionBreak(section) => out.push(FlowItem::SectionBreak(section.clone())),
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

fn fill_circle_color(surface: &mut Surface<'_>, cx: f32, cy: f32, radius: f32, color: rgb::Color) {
    if radius <= 0.0 {
        return;
    }
    let mut pb = PathBuilder::new();
    let steps = 28usize;
    for step in 0..=steps {
        let angle = std::f32::consts::TAU * step as f32 / steps as f32;
        let x = cx + radius * angle.cos();
        let y = cy + radius * angle.sin();
        if step == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
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

fn fill_triangle_color(
    surface: &mut Surface<'_>,
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    color: rgb::Color,
) {
    let mut pb = PathBuilder::new();
    pb.move_to(p1.0, p1.1);
    pb.line_to(p2.0, p2.1);
    pb.line_to(p3.0, p3.1);
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

fn fill_quad_color(
    surface: &mut Surface<'_>,
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    p4: (f32, f32),
    color: rgb::Color,
) {
    let mut pb = PathBuilder::new();
    pb.move_to(p1.0, p1.1);
    pb.line_to(p2.0, p2.1);
    pb.line_to(p3.0, p3.1);
    pb.line_to(p4.0, p4.1);
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

fn fill_chart_bar_shape(
    surface: &mut Surface<'_>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    shape: ChartShape,
    color: rgb::Color,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    match shape {
        ChartShape::Cylinder => {
            let radius = (h * 0.5).min(w * 0.5);
            fill_rect_color(
                surface,
                x + radius * 0.5,
                y,
                (w - radius).max(1.0),
                h,
                color,
            );
            fill_circle_color(surface, x + radius, y + h * 0.5, radius, color);
            fill_circle_color(surface, x + w - radius, y + h * 0.5, radius, color);
        }
        ChartShape::Cone
        | ChartShape::ConeToMax
        | ChartShape::Pyramid
        | ChartShape::PyramidToMax => {
            fill_triangle_color(surface, (x, y), (x, y + h), (x + w, y + h * 0.5), color);
        }
        ChartShape::Box => fill_rect_color(surface, x, y, w, h, color),
    }
}

fn fill_chart_column_shape(
    surface: &mut Surface<'_>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    shape: ChartShape,
    color: rgb::Color,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    match shape {
        ChartShape::Cylinder => {
            let radius = (w * 0.5).min(h * 0.5);
            fill_rect_color(
                surface,
                x,
                y + radius * 0.5,
                w,
                (h - radius).max(1.0),
                color,
            );
            fill_circle_color(surface, x + w * 0.5, y + radius, radius, color);
            fill_circle_color(surface, x + w * 0.5, y + h - radius, radius, color);
        }
        ChartShape::Cone
        | ChartShape::ConeToMax
        | ChartShape::Pyramid
        | ChartShape::PyramidToMax => {
            fill_triangle_color(surface, (x + w * 0.5, y), (x, y + h), (x + w, y + h), color);
        }
        ChartShape::Box => fill_rect_color(surface, x, y, w, h, color),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BorderRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn cell_border_rects(
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    paints: CellBorderPaints,
) -> Option<[Option<BorderRect>; 4]> {
    if !left.is_finite()
        || !top.is_finite()
        || !right.is_finite()
        || !bottom.is_finite()
        || right <= left
        || bottom <= top
    {
        return None;
    }

    let width = |paint: Option<TableBorderPaint>| {
        paint.map_or(0.0, |paint| {
            if paint.width.is_finite() && paint.width > 0.0 {
                paint.width
            } else {
                f32::NAN
            }
        })
    };
    let top_width = width(paints.top);
    let left_width = width(paints.left);
    let bottom_width = width(paints.bottom);
    let right_width = width(paints.right);
    if [top_width, left_width, bottom_width, right_width]
        .into_iter()
        .any(|width| !width.is_finite())
    {
        return None;
    }

    let horizontal_x = left - left_width * 0.5;
    let horizontal_width = right - left + (left_width + right_width) * 0.5;
    let vertical_y = top - top_width * 0.5;
    let vertical_height = bottom - top + (top_width + bottom_width) * 0.5;
    Some([
        paints.top.map(|_| BorderRect {
            x: horizontal_x,
            y: top - top_width * 0.5,
            w: horizontal_width,
            h: top_width,
        }),
        paints.bottom.map(|_| BorderRect {
            x: horizontal_x,
            y: bottom - bottom_width * 0.5,
            w: horizontal_width,
            h: bottom_width,
        }),
        paints.left.map(|_| BorderRect {
            x: left - left_width * 0.5,
            y: vertical_y,
            w: left_width,
            h: vertical_height,
        }),
        paints.right.map(|_| BorderRect {
            x: right - right_width * 0.5,
            y: vertical_y,
            w: right_width,
            h: vertical_height,
        }),
    ])
}

fn row_vertical_border_lines(row: &RowLayout, x_offset: f32) -> Vec<VerticalBorderLine> {
    let mut lines = Vec::with_capacity(row.cells.len().saturating_mul(2));
    for cell in &row.cells {
        for (x, side) in [
            (cell.x, cell.border_edges.left),
            (cell.right, cell.border_edges.right),
        ] {
            if let Some(side) = side {
                lines.push(VerticalBorderLine {
                    x: x_offset + x,
                    paint: row.border.get(side),
                });
            }
        }
    }
    lines.sort_by(|left, right| left.x.total_cmp(&right.x));
    lines.dedup_by(|left, right| left.x == right.x);
    lines
}

fn top_horizontal_paint_at(row: &RowLayout, x_offset: f32, x: f32) -> Option<TableBorderPaint> {
    let local_x = x - x_offset;
    row.cells.iter().find_map(|cell| {
        let side = cell.border_edges.top?;
        (local_x >= cell.x && local_x <= cell.right).then(|| row.border.get(side))
    })
}

fn terminal_vertical_junctions(
    previous: &[VerticalBorderLine],
    current: &RowLayout,
    current_vertical: &[VerticalBorderLine],
    x_offset: f32,
) -> Vec<(VerticalBorderLine, f32)> {
    previous
        .iter()
        .copied()
        .filter(|line| !current_vertical.iter().any(|current| current.x == line.x))
        .filter_map(|line| {
            let horizontal = top_horizontal_paint_at(current, x_offset, line.x)?;
            (line.paint != horizontal).then_some((line, horizontal.width))
        })
        .collect()
}

#[cfg(test)]
fn table_border_rects(
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    requested_width: f32,
) -> Option<[BorderRect; 4]> {
    let paint = TableBorderPaint {
        color: rgb::Color::black(),
        width: requested_width,
    };
    cell_border_rects(left, top, right, bottom, CellBorderPaints::uniform(paint))
        .map(|rects| rects.map(|rect| rect.expect("uniform paint has every edge")))
}

fn draw_horizontal_table_borders(
    surface: &mut Surface<'_>,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    paints: CellBorderPaints,
) {
    if let Some(rects) = cell_border_rects(left, top, right, bottom, paints) {
        for (rect, paint) in [(rects[0], paints.top), (rects[1], paints.bottom)] {
            let (Some(rect), Some(paint)) = (rect, paint) else {
                continue;
            };
            fill_rect_color(surface, rect.x, rect.y, rect.w, rect.h, paint.color);
        }
    }
}

fn draw_vertical_table_borders(
    surface: &mut Surface<'_>,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    paints: CellBorderPaints,
) {
    if let Some(rects) = cell_border_rects(left, top, right, bottom, paints) {
        for (rect, paint) in [(rects[2], paints.left), (rects[3], paints.right)] {
            let (Some(rect), Some(paint)) = (rect, paint) else {
                continue;
            };
            fill_rect_color(surface, rect.x, rect.y, rect.w, rect.h, paint.color);
        }
    }
}

fn draw_terminal_vertical_junction(
    surface: &mut Surface<'_>,
    top: f32,
    line: VerticalBorderLine,
    horizontal_width: f32,
) {
    fill_rect_color(
        surface,
        line.x - line.paint.width * 0.5,
        top - horizontal_width * 0.5,
        line.paint.width,
        horizontal_width,
        line.paint.color,
    );
}

fn draw_table_cell_background_and_borders(
    surface: &mut Surface<'_>,
    cell: &CellBox,
    x_offset: f32,
    top: f32,
    bottom: f32,
    row_height: f32,
    border: TableBorderPaints,
) {
    let left = x_offset + cell.x;
    let right = x_offset + cell.right;
    if let Some(fill) = cell.shading {
        fill_rect_color(surface, left, top, cell.width, row_height, fill);
    }
    let paints = CellBorderPaints::resolve(cell.border_edges, border);
    draw_horizontal_table_borders(surface, left, top, right, bottom, paints);
    draw_vertical_table_borders(surface, left, top, right, bottom, paints);
}

fn draw_border_color(surface: &mut Surface<'_>, x: f32, y: f32, w: f32, h: f32, color: rgb::Color) {
    fill_rect_color(surface, x, y, w, BORDER, color);
    fill_rect_color(surface, x, y + h - BORDER, w, BORDER, color);
    fill_rect_color(surface, x, y, BORDER, h, color);
    fill_rect_color(surface, x + w - BORDER, y, BORDER, h, color);
}

fn cell_line_origin(cell_x: f32, insets: CellInsets, line: &LineLayout) -> f32 {
    cell_x + insets.left + line.x_indent
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CellContentPlacement {
    x: f32,
    top: f32,
    row_height: f32,
}

fn cell_lines_extent(lines: &[LineLayout]) -> f32 {
    lines.iter().map(LineLayout::cell_extent).sum()
}

fn cell_vertical_offset(cell: &CellBox, row_height: f32) -> f32 {
    let available = row_height - cell.insets.top - cell.insets.bottom;
    let slack = (available - cell_lines_extent(&cell.lines)).max(0.0);
    match cell.valign {
        VCell::Top => 0.0,
        VCell::Center => slack * 0.5,
        VCell::Bottom => slack,
    }
}

fn draw_table_cell_content(
    surface: &mut Surface<'_>,
    cell: CellBox,
    placement: CellContentPlacement,
    page_number: usize,
    cx: &mut TextCx<'_>,
    page_links: &mut Vec<(f32, f32, f32, f32, Rc<str>)>,
) {
    let offset = cell_vertical_offset(&cell, placement.row_height);
    let mut line_top = placement.top + cell.insets.top + offset;
    for line in cell.lines {
        line_top += line.cell_spacing.before;
        let baseline = line_top + line.baseline;
        let line_height = line.height;
        let after = line.cell_spacing.after;
        let line_x = cell_line_origin(placement.x, cell.insets, &line);
        draw_line_background(surface, &line, line_x, line_top);
        for run in line.runs {
            if let Some(url) = run.link.clone() {
                let left = line_x + run.x;
                page_links.push((
                    left,
                    line_top,
                    left + run.width(),
                    line_top + line_height,
                    url,
                ));
            }
            draw_run_with_page_context(surface, run, line_x, baseline, page_number, cx);
        }
        line_top += line_height + after;
    }
}

fn draw_line_background(surface: &mut Surface<'_>, line: &LineLayout, x_abs: f32, top: f32) {
    if let Some(background) = line.background {
        fill_rect_color(
            surface,
            x_abs,
            top,
            background.width,
            line.height,
            background.color,
        );
    }
}

fn draw_floating_shape_overlay(
    surface: &mut Surface<'_>,
    overlay: &FloatingShapeOverlay,
    cx: &mut TextCx<'_>,
) {
    fill_rect_color(
        surface,
        overlay.x,
        overlay.y,
        overlay.w,
        overlay.h,
        rgb::Color::new(0xF6, 0xF8, 0xFA),
    );
    draw_border_color(
        surface,
        overlay.x,
        overlay.y,
        overlay.w,
        overlay.h,
        rgb::Color::new(0x5D, 0x6B, 0x78),
    );
    draw_chart_text(
        surface,
        &overlay.label,
        overlay.x + 4.0,
        overlay.y + 4.0,
        (overlay.w - 8.0).max(1.0),
        ChartTextStyle {
            size_pt: 7.5,
            bold: false,
            align: Alignment::Start,
            color: Color::rgb(0x32, 0x3A, 0x43),
        },
        cx,
    );
}

/// The text style for a single chart label: point size, weight, alignment, and
/// fill color. These four are always set together at a `draw_chart_text` call.
#[derive(Clone, Copy)]
struct ChartTextStyle {
    size_pt: f32,
    bold: bool,
    align: Alignment,
    color: Color,
}

fn draw_chart_text(
    surface: &mut Surface<'_>,
    text: &str,
    x: f32,
    y: f32,
    width: f32,
    style: ChartTextStyle,
    cx: &mut TextCx<'_>,
) -> f32 {
    if text.trim().is_empty() || width <= 0.0 {
        return 0.0;
    }
    let size_half_pt = (style.size_pt * 2.0).round().max(1.0) as u16;
    let props = CharProps {
        bold: style.bold,
        size_half_pt: Some(size_half_pt),
        color: Some(style.color),
        ..CharProps::default()
    };
    let mut consumed = 0.0;
    for line in shape(
        text,
        StyledText::plain(&[(0, text.len(), props)]),
        None,
        style.align,
        width,
        cx,
    )
    .into_iter()
    .take(2)
    {
        let baseline = y + consumed + line.baseline;
        draw_line_background(surface, &line, x + line.x_indent, y + consumed);
        for run in line.runs {
            draw_run(surface, run, x + line.x_indent, baseline);
        }
        consumed += line.height;
    }
    consumed
}

fn chart_series_color(index: usize) -> rgb::Color {
    const COLORS: [(u8, u8, u8); 6] = [
        (0x2F, 0x6F, 0xD6),
        (0xD9, 0x4E, 0x4E),
        (0x27, 0x9A, 0x68),
        (0x9B, 0x5D, 0xC8),
        (0xD8, 0x8A, 0x25),
        (0x36, 0x8C, 0xA8),
    ];
    let (r, g, b) = COLORS[index % COLORS.len()];
    rgb::Color::new(r, g, b)
}

fn chart_value_range(chart: &Chart) -> (f64, f64) {
    let mut min = 0.0;
    let mut max = 0.0;
    for value in chart
        .series
        .iter()
        .flat_map(|series| series.values.iter().copied())
        .filter(|value| value.is_finite())
    {
        if value < min {
            min = value;
        }
        if value > max {
            max = value;
        }
    }
    if min == max {
        if max == 0.0 {
            max = 1.0;
        } else if max > 0.0 {
            min = 0.0;
        } else {
            max = 0.0;
        }
    }
    (min, max)
}

fn stacked_chart_max(chart: &Chart, category_count: usize) -> f64 {
    let mut max = 0.0;
    for category_index in 0..category_count {
        let total = stacked_category_total(chart, category_index);
        if total > max {
            max = total;
        }
    }
    max.max(1.0)
}

fn stacked_category_total(chart: &Chart, category_index: usize) -> f64 {
    chart
        .series
        .iter()
        .filter_map(|series| series.values.get(category_index).copied())
        .filter(|value| value.is_finite() && *value > 0.0)
        .sum()
}

fn chart_bubble_size_range(chart: &Chart) -> (f64, f64) {
    let mut max = 1.0;
    for size in chart
        .series
        .iter()
        .flat_map(|series| series.bubble_sizes.iter().copied())
        .filter(|size| size.is_finite() && *size > 0.0)
    {
        if size > max {
            max = size;
        }
    }
    (1.0, max)
}

fn format_chart_tick(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    if (value.fract()).abs() < 0.001 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.1}")
    }
}

fn fill_line_segment(
    surface: &mut Surface<'_>,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    width: f32,
    color: rgb::Color,
) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt();
    if len <= 0.01 {
        fill_rect_color(
            surface,
            x1 - width * 0.5,
            y1 - width * 0.5,
            width,
            width,
            color,
        );
        return;
    }
    let px = -dy / len * width * 0.5;
    let py = dx / len * width * 0.5;
    let mut pb = PathBuilder::new();
    pb.move_to(x1 + px, y1 + py);
    pb.line_to(x2 + px, y2 + py);
    pb.line_to(x2 - px, y2 - py);
    pb.line_to(x1 - px, y1 - py);
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

fn fill_area_shape(
    surface: &mut Surface<'_>,
    points: &[(f32, f32)],
    baseline_y: f32,
    color: rgb::Color,
) {
    let Some((first_x, _)) = points.first().copied() else {
        return;
    };
    let Some((last_x, _)) = points.last().copied() else {
        return;
    };
    let mut pb = PathBuilder::new();
    pb.move_to(first_x, baseline_y);
    for (x, y) in points {
        pb.line_to(*x, *y);
    }
    pb.line_to(last_x, baseline_y);
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

#[derive(Clone, Copy)]
struct ChartRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn fill_pie_slice(
    surface: &mut Surface<'_>,
    cx: f32,
    cy: f32,
    radius: f32,
    start_angle: f32,
    sweep: f32,
    color: rgb::Color,
) {
    if radius <= 0.0 || sweep.abs() <= 0.0001 {
        return;
    }
    let steps = ((sweep.abs() / (std::f32::consts::PI / 24.0)).ceil() as usize).clamp(2, 96);
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy);
    for step in 0..=steps {
        let angle = start_angle + sweep * step as f32 / steps as f32;
        pb.line_to(cx + angle.cos() * radius, cy + angle.sin() * radius);
    }
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

/// An annular sector (ring slice): center, inner/outer radii, and the start angle
/// and sweep. These six geometry values always describe one slice together.
#[derive(Clone, Copy)]
struct RingSlice {
    cx: f32,
    cy: f32,
    inner_radius: f32,
    outer_radius: f32,
    start_angle: f32,
    sweep: f32,
}

fn fill_ring_slice(surface: &mut Surface<'_>, ring: RingSlice, color: rgb::Color) {
    let RingSlice {
        cx,
        cy,
        inner_radius,
        outer_radius,
        start_angle,
        sweep,
    } = ring;
    if outer_radius <= inner_radius || sweep.abs() <= 0.0001 {
        return;
    }
    let steps = ((sweep.abs() / (std::f32::consts::PI / 24.0)).ceil() as usize).clamp(2, 96);
    let mut pb = PathBuilder::new();
    for step in 0..=steps {
        let angle = start_angle + sweep * step as f32 / steps as f32;
        let x = cx + angle.cos() * outer_radius;
        let y = cy + angle.sin() * outer_radius;
        if step == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    for step in (0..=steps).rev() {
        let angle = start_angle + sweep * step as f32 / steps as f32;
        pb.line_to(
            cx + angle.cos() * inner_radius,
            cy + angle.sin() * inner_radius,
        );
    }
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

fn draw_pie_chart(
    surface: &mut Surface<'_>,
    chart: &Chart,
    rect: ChartRect,
    doughnut: bool,
    exploded: bool,
) {
    let Some(series) = chart.series.first() else {
        return;
    };
    let values = chart
        .categories
        .iter()
        .enumerate()
        .map(|(index, _)| {
            series
                .values
                .get(index)
                .copied()
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(0.0)
        })
        .collect::<Vec<_>>();
    let total: f64 = values.iter().sum();
    if total <= 0.0 {
        return;
    }
    let radius = (rect.w.min(rect.h) * 0.42).max(1.0);
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let explosion = if exploded { radius * 0.08 } else { 0.0 };
    let mut angle = -std::f32::consts::FRAC_PI_2;
    for (index, value) in values.iter().enumerate() {
        if *value <= 0.0 {
            continue;
        }
        let sweep = (*value / total) as f32 * std::f32::consts::TAU;
        let mid_angle = angle + sweep * 0.5;
        let slice_cx = cx + mid_angle.cos() * explosion;
        let slice_cy = cy + mid_angle.sin() * explosion;
        fill_pie_slice(
            surface,
            slice_cx,
            slice_cy,
            radius,
            angle,
            sweep,
            chart_series_color(index),
        );
        angle += sweep;
    }
    if doughnut {
        fill_pie_slice(
            surface,
            cx,
            cy,
            radius * 0.52,
            -std::f32::consts::FRAC_PI_2,
            std::f32::consts::TAU,
            rgb::Color::new(0xFF, 0xFF, 0xFF),
        );
    }
}

fn draw_radar_chart(
    surface: &mut Surface<'_>,
    chart: &Chart,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tcx: &mut TextCx<'_>,
) {
    if chart.categories.is_empty() || chart.series.is_empty() {
        return;
    }
    let grid = rgb::Color::new(0xE1, 0xE5, 0xEA);
    let axis = rgb::Color::new(0x5D, 0x66, 0x70);
    let max_value = chart
        .series
        .iter()
        .flat_map(|series| series.values.iter().copied())
        .filter(|value| value.is_finite() && *value > 0.0)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let cx = x + w * 0.5;
    let cy = y + h * 0.5;
    let radius = (w.min(h) * 0.36).max(1.0);
    let label_radius = radius + 9.0;
    let count = chart.categories.len();
    let point_at = |index: usize, value: f64| {
        let angle =
            -std::f32::consts::FRAC_PI_2 + index as f32 / count as f32 * std::f32::consts::TAU;
        let frac = (value.max(0.0) / max_value).clamp(0.0, 1.0) as f32;
        (
            cx + angle.cos() * radius * frac,
            cy + angle.sin() * radius * frac,
        )
    };
    for ring in 1..=4 {
        let frac = ring as f64 / 4.0;
        let ring_points = (0..count)
            .map(|index| point_at(index, max_value * frac))
            .collect::<Vec<_>>();
        for index in 0..ring_points.len() {
            let (x1, y1) = ring_points[index];
            let (x2, y2) = ring_points[(index + 1) % ring_points.len()];
            fill_line_segment(surface, x1, y1, x2, y2, 0.45, grid);
        }
    }
    for (index, category) in chart.categories.iter().enumerate() {
        let (spoke_x, spoke_y) = point_at(index, max_value);
        fill_line_segment(surface, cx, cy, spoke_x, spoke_y, 0.45, axis);
        let angle =
            -std::f32::consts::FRAC_PI_2 + index as f32 / count as f32 * std::f32::consts::TAU;
        let label_x = cx + angle.cos() * label_radius;
        let label_y = cy + angle.sin() * label_radius;
        draw_chart_text(
            surface,
            category,
            label_x - 28.0,
            label_y - 5.0,
            56.0,
            ChartTextStyle {
                size_pt: 7.5,
                bold: false,
                align: Alignment::Center,
                color: Color::rgb(0x25, 0x2D, 0x36),
            },
            tcx,
        );
    }
    for (series_index, series) in chart.series.iter().enumerate() {
        let color = chart_series_color(series_index);
        let points = (0..count)
            .map(|index| {
                let value = series
                    .values
                    .get(index)
                    .copied()
                    .filter(|value| value.is_finite())
                    .unwrap_or(0.0);
                point_at(index, value)
            })
            .collect::<Vec<_>>();
        for index in 0..points.len() {
            let (x1, y1) = points[index];
            let (x2, y2) = points[(index + 1) % points.len()];
            fill_line_segment(surface, x1, y1, x2, y2, 1.5, color);
            fill_rect_color(surface, x1 - 2.0, y1 - 2.0, 4.0, 4.0, color);
        }
    }
}

fn draw_waterfall_chart(
    surface: &mut Surface<'_>,
    chart: &Chart,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tcx: &mut TextCx<'_>,
) {
    let Some(series) = chart.series.first() else {
        return;
    };
    let count = chart.categories.len().max(series.values.len()).max(1);
    let mut segments = Vec::with_capacity(count);
    let mut running = 0.0;
    let mut min_value = 0.0f64;
    let mut max_value = 0.0f64;
    for index in 0..count {
        let value = series
            .values
            .get(index)
            .copied()
            .filter(|value| value.is_finite())
            .unwrap_or(0.0);
        let is_total = index == 0 || index + 1 == count;
        let start = if is_total { 0.0 } else { running };
        let end = if is_total { value } else { running + value };
        running = end;
        min_value = min_value.min(start.min(end));
        max_value = max_value.max(start.max(end));
        segments.push((start, end, is_total));
    }
    let range = (max_value - min_value).max(1.0);
    let value_y = |value: f64| y + h - (((value - min_value) / range) as f32 * h);
    let zero_y = value_y(0.0).clamp(y, y + h);
    for tick in 0..=4 {
        let frac = tick as f32 / 4.0;
        let y_tick = y + h - frac * h;
        fill_rect_color(
            surface,
            x,
            y_tick,
            w,
            0.35,
            rgb::Color::new(0xE1, 0xE5, 0xEA),
        );
        let value = min_value + (max_value - min_value) * tick as f64 / 4.0;
        let label = format_chart_tick(value);
        draw_chart_text(
            surface,
            &label,
            x - 48.0,
            y_tick - 5.0,
            42.0,
            ChartTextStyle {
                size_pt: 7.5,
                bold: false,
                align: Alignment::End,
                color: Color::rgb(0x4C, 0x55, 0x5F),
            },
            tcx,
        );
    }
    fill_rect_color(
        surface,
        x,
        zero_y,
        w,
        0.8,
        rgb::Color::new(0x5D, 0x66, 0x70),
    );
    fill_rect_color(surface, x, y, 0.8, h, rgb::Color::new(0x5D, 0x66, 0x70));

    let band_w = w / count as f32;
    let bar_w = (band_w * 0.58).max(2.0);
    for (index, (start, end, is_total)) in segments.iter().copied().enumerate() {
        let left = x + index as f32 * band_w + (band_w - bar_w) * 0.5;
        let y_start = value_y(start).clamp(y, y + h);
        let y_end = value_y(end).clamp(y, y + h);
        let top = y_start.min(y_end);
        let height = (y_start - y_end).abs().max(1.0);
        let color = if is_total {
            rgb::Color::new(0x3B, 0x6E, 0xA8)
        } else if end >= start {
            rgb::Color::new(0x32, 0x8A, 0x62)
        } else {
            rgb::Color::new(0xC7, 0x52, 0x4A)
        };
        fill_rect_color(surface, left, top, bar_w, height, color);
        if index > 0 {
            let prev_x = x + index as f32 * band_w - (band_w - bar_w) * 0.5;
            fill_rect_color(
                surface,
                prev_x,
                y_start,
                (left - prev_x).max(1.0),
                0.5,
                rgb::Color::new(0x9A, 0xA4, 0xAE),
            );
        }
        if let Some(category) = chart.categories.get(index) {
            draw_chart_text(
                surface,
                category,
                x + index as f32 * band_w,
                y + h + 3.0,
                band_w,
                ChartTextStyle {
                    size_pt: 8.0,
                    bold: false,
                    align: Alignment::Center,
                    color: Color::rgb(0x25, 0x2D, 0x36),
                },
                tcx,
            );
        }
    }
}

fn draw_treemap_chart(
    surface: &mut Surface<'_>,
    chart: &Chart,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tcx: &mut TextCx<'_>,
) {
    let Some(series) = chart.series.first() else {
        return;
    };
    let values: Vec<f64> = series
        .values
        .iter()
        .copied()
        .map(|value| if value.is_finite() { value.abs() } else { 0.0 })
        .collect();
    let mut remaining: f64 = values.iter().sum::<f64>().max(1.0);
    let mut rect_x = x;
    let mut rect_y = y;
    let mut rect_w = w;
    let mut rect_h = h;
    for (index, value) in values.iter().copied().enumerate() {
        if rect_w <= 1.0 || rect_h <= 1.0 {
            break;
        }
        let is_last = index + 1 == values.len();
        let share = if is_last {
            1.0
        } else {
            (value / remaining).clamp(0.0, 1.0) as f32
        };
        let (cell_x, cell_y, cell_w, cell_h) = if rect_w >= rect_h {
            let cell_w = if is_last { rect_w } else { rect_w * share };
            let cell = (rect_x, rect_y, cell_w, rect_h);
            rect_x += cell_w;
            rect_w = (rect_w - cell_w).max(0.0);
            cell
        } else {
            let cell_h = if is_last { rect_h } else { rect_h * share };
            let cell = (rect_x, rect_y, rect_w, cell_h);
            rect_y += cell_h;
            rect_h = (rect_h - cell_h).max(0.0);
            cell
        };
        remaining = (remaining - value).max(0.0);
        let color = chart_series_color(index);
        fill_rect_color(surface, cell_x, cell_y, cell_w, cell_h, color);
        fill_rect_color(
            surface,
            cell_x,
            cell_y,
            cell_w,
            0.75,
            rgb::Color::new(0xFF, 0xFF, 0xFF),
        );
        fill_rect_color(
            surface,
            cell_x,
            cell_y + cell_h - 0.75,
            cell_w,
            0.75,
            rgb::Color::new(0xFF, 0xFF, 0xFF),
        );
        fill_rect_color(
            surface,
            cell_x,
            cell_y,
            0.75,
            cell_h,
            rgb::Color::new(0xFF, 0xFF, 0xFF),
        );
        fill_rect_color(
            surface,
            cell_x + cell_w - 0.75,
            cell_y,
            0.75,
            cell_h,
            rgb::Color::new(0xFF, 0xFF, 0xFF),
        );
        if let Some(category) = chart.categories.get(index) {
            draw_chart_text(
                surface,
                category,
                cell_x + 3.0,
                cell_y + 3.0,
                (cell_w - 6.0).max(1.0),
                ChartTextStyle {
                    size_pt: 8.0,
                    bold: false,
                    align: Alignment::Start,
                    color: Color::rgb(0xFF, 0xFF, 0xFF),
                },
                tcx,
            );
        }
    }
}

fn draw_sunburst_chart(surface: &mut Surface<'_>, chart: &Chart, x: f32, y: f32, w: f32, h: f32) {
    let Some(series) = chart.series.first() else {
        return;
    };
    let values = chart
        .categories
        .iter()
        .enumerate()
        .map(|(index, _)| {
            series
                .values
                .get(index)
                .copied()
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(0.0)
        })
        .collect::<Vec<_>>();
    let total: f64 = values.iter().sum();
    if total <= 0.0 {
        return;
    }
    let radius = (w.min(h) * 0.44).max(1.0);
    let cx = x + w * 0.5;
    let cy = y + h * 0.5;
    fill_circle_color(
        surface,
        cx,
        cy,
        radius * 0.38,
        rgb::Color::new(0xD8, 0xDF, 0xE7),
    );
    let mut angle = -std::f32::consts::FRAC_PI_2;
    for (index, value) in values.iter().enumerate() {
        if *value <= 0.0 {
            continue;
        }
        let sweep = (*value / total) as f32 * std::f32::consts::TAU;
        fill_ring_slice(
            surface,
            RingSlice {
                cx,
                cy,
                inner_radius: radius * 0.44,
                outer_radius: radius,
                start_angle: angle,
                sweep,
            },
            chart_series_color(index),
        );
        fill_ring_slice(
            surface,
            RingSlice {
                cx,
                cy,
                inner_radius: radius * 0.38,
                outer_radius: radius * 0.43,
                start_angle: angle,
                sweep,
            },
            rgb::Color::new(0xFF, 0xFF, 0xFF),
        );
        angle += sweep;
    }
}

fn draw_box_whisker_chart(
    surface: &mut Surface<'_>,
    chart: &Chart,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tcx: &mut TextCx<'_>,
) {
    let Some(series) = chart.series.first() else {
        return;
    };
    let mut values = series
        .values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if values.is_empty() {
        return;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min = *values.first().unwrap_or(&0.0);
    let max = *values.last().unwrap_or(&0.0);
    let q1 = percentile(&values, 0.25);
    let median = percentile(&values, 0.5);
    let q3 = percentile(&values, 0.75);
    let range = (max - min).max(1.0);
    let value_y = |value: f64| y + h - (((value - min) / range) as f32 * h);
    for tick in 0..=4 {
        let frac = tick as f32 / 4.0;
        let y_tick = y + h - frac * h;
        fill_rect_color(
            surface,
            x,
            y_tick,
            w,
            0.35,
            rgb::Color::new(0xE1, 0xE5, 0xEA),
        );
        let value = min + (max - min) * tick as f64 / 4.0;
        let label = format_chart_tick(value);
        draw_chart_text(
            surface,
            &label,
            x - 48.0,
            y_tick - 5.0,
            42.0,
            ChartTextStyle {
                size_pt: 7.5,
                bold: false,
                align: Alignment::End,
                color: Color::rgb(0x4C, 0x55, 0x5F),
            },
            tcx,
        );
    }
    let center_x = x + w * 0.5;
    let box_w = (w * 0.28).clamp(32.0, 90.0);
    let q1_y = value_y(q1).clamp(y, y + h);
    let q3_y = value_y(q3).clamp(y, y + h);
    let min_y = value_y(min).clamp(y, y + h);
    let max_y = value_y(max).clamp(y, y + h);
    let median_y = value_y(median).clamp(y, y + h);
    let box_top = q3_y.min(q1_y);
    let box_h = (q1_y - q3_y).abs().max(1.0);
    let line = rgb::Color::new(0x35, 0x43, 0x52);
    fill_rect_color(surface, center_x - 0.5, max_y, 1.0, min_y - max_y, line);
    fill_rect_color(
        surface,
        center_x - box_w * 0.35,
        max_y,
        box_w * 0.7,
        1.0,
        line,
    );
    fill_rect_color(
        surface,
        center_x - box_w * 0.35,
        min_y,
        box_w * 0.7,
        1.0,
        line,
    );
    fill_rect_color(
        surface,
        center_x - box_w * 0.5,
        box_top,
        box_w,
        box_h,
        rgb::Color::new(0x7A, 0xA0, 0xC8),
    );
    fill_rect_color(surface, center_x - box_w * 0.5, box_top, box_w, 1.0, line);
    fill_rect_color(
        surface,
        center_x - box_w * 0.5,
        box_top + box_h,
        box_w,
        1.0,
        line,
    );
    fill_rect_color(surface, center_x - box_w * 0.5, box_top, 1.0, box_h, line);
    fill_rect_color(surface, center_x + box_w * 0.5, box_top, 1.0, box_h, line);
    fill_rect_color(surface, center_x - box_w * 0.5, median_y, box_w, 1.3, line);
}

fn percentile(sorted: &[f64], frac: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = (sorted.len().saturating_sub(1) as f64 * frac).clamp(0.0, sorted.len() as f64 - 1.0);
    let lower = pos.floor() as usize;
    let upper = pos.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let weight = pos - lower as f64;
        sorted[lower] * (1.0 - weight) + sorted[upper] * weight
    }
}

fn draw_funnel_chart(
    surface: &mut Surface<'_>,
    chart: &Chart,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tcx: &mut TextCx<'_>,
) {
    let Some(series) = chart.series.first() else {
        return;
    };
    let values = chart
        .categories
        .iter()
        .enumerate()
        .map(|(index, _)| {
            series
                .values
                .get(index)
                .copied()
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(0.0)
        })
        .collect::<Vec<_>>();
    let Some(max_value) = values.iter().copied().reduce(f64::max) else {
        return;
    };
    if max_value <= 0.0 {
        return;
    }
    let count = values.len().max(1);
    let stage_h = h / count as f32;
    let center_x = x + w * 0.5;
    for (index, value) in values.iter().copied().enumerate() {
        let next = values.get(index + 1).copied().unwrap_or(value * 0.72);
        let top_w = (value / max_value) as f32 * w * 0.88;
        let bottom_w = (next / max_value) as f32 * w * 0.88;
        let top_y = y + index as f32 * stage_h + 1.0;
        let bottom_y = y + (index + 1) as f32 * stage_h - 1.0;
        fill_quad_color(
            surface,
            (center_x - top_w * 0.5, top_y),
            (center_x + top_w * 0.5, top_y),
            (center_x + bottom_w * 0.5, bottom_y),
            (center_x - bottom_w * 0.5, bottom_y),
            chart_series_color(index),
        );
        if let Some(category) = chart.categories.get(index) {
            draw_chart_text(
                surface,
                category,
                center_x - top_w.max(bottom_w) * 0.45,
                top_y + stage_h * 0.28,
                top_w.max(bottom_w) * 0.9,
                ChartTextStyle {
                    size_pt: 8.0,
                    bold: false,
                    align: Alignment::Center,
                    color: Color::rgb(0xFF, 0xFF, 0xFF),
                },
                tcx,
            );
        }
    }
}

fn draw_authored_chart(
    surface: &mut Surface<'_>,
    chart: &Chart,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tcx: &mut TextCx<'_>,
) {
    let border = rgb::Color::new(0xA7, 0xB0, 0xBA);
    let axis = rgb::Color::new(0x5D, 0x66, 0x70);
    let grid = rgb::Color::new(0xE1, 0xE5, 0xEA);
    fill_rect_color(surface, x, y, w, h, rgb::Color::new(0xFF, 0xFF, 0xFF));
    fill_rect_color(surface, x, y, w, BORDER, border);
    fill_rect_color(surface, x, y + h - BORDER, w, BORDER, border);
    fill_rect_color(surface, x, y, BORDER, h, border);
    fill_rect_color(surface, x + w - BORDER, y, BORDER, h, border);

    let mut content_top = y + 8.0;
    if let Some(title) = chart.title.as_deref() {
        let used = draw_chart_text(
            surface,
            title,
            x + 8.0,
            content_top,
            (w - 16.0).max(1.0),
            ChartTextStyle {
                size_pt: 11.0,
                bold: true,
                align: Alignment::Center,
                color: Color::rgb(0x1E, 0x2A, 0x36),
            },
            tcx,
        );
        content_top += used + 4.0;
    }

    let label_w = (w * 0.24).clamp(54.0, 110.0);
    let legend_h = 18.0;
    let plot_left = x + label_w + 10.0;
    let plot_right = x + w - 12.0;
    let plot_top = content_top;
    let plot_bottom = y + h - legend_h - 18.0;
    let plot_w = (plot_right - plot_left).max(1.0);
    let plot_h = (plot_bottom - plot_top).max(1.0);
    if plot_w <= 8.0 || plot_h <= 8.0 {
        return;
    }

    if matches!(
        chart.kind,
        ChartKind::Pie
            | ChartKind::ExplodedPie
            | ChartKind::Pie3D
            | ChartKind::ExplodedPie3D
            | ChartKind::PieOfPie
            | ChartKind::BarOfPie
            | ChartKind::Doughnut
            | ChartKind::ExplodedDoughnut
    ) {
        draw_pie_chart(
            surface,
            chart,
            ChartRect {
                x: plot_left,
                y: plot_top,
                w: plot_w,
                h: plot_h,
            },
            matches!(
                chart.kind,
                ChartKind::Doughnut | ChartKind::ExplodedDoughnut
            ),
            matches!(
                chart.kind,
                ChartKind::ExplodedPie | ChartKind::ExplodedPie3D | ChartKind::ExplodedDoughnut
            ),
        );
        let mut legend_x = plot_left;
        let legend_y = y + h - 14.0;
        for (index, category) in chart.categories.iter().enumerate() {
            if legend_x >= plot_right - 20.0 {
                break;
            }
            fill_rect_color(
                surface,
                legend_x,
                legend_y + 3.0,
                6.0,
                6.0,
                chart_series_color(index),
            );
            let used = draw_chart_text(
                surface,
                category,
                legend_x + 9.0,
                legend_y,
                (plot_right - legend_x - 9.0).max(1.0),
                ChartTextStyle {
                    size_pt: 8.0,
                    bold: false,
                    align: Alignment::Start,
                    color: Color::rgb(0x25, 0x2D, 0x36),
                },
                tcx,
            );
            legend_x += 9.0 + (category.chars().count() as f32 * 4.8).max(used * 3.0) + 12.0;
        }
        return;
    }

    if matches!(
        chart.kind,
        ChartKind::Radar | ChartKind::RadarWithMarkers | ChartKind::FilledRadar
    ) {
        draw_radar_chart(surface, chart, plot_left, plot_top, plot_w, plot_h, tcx);
        let mut legend_x = plot_left;
        let legend_y = y + h - 14.0;
        for (index, series) in chart.series.iter().enumerate() {
            if legend_x >= plot_right - 20.0 {
                break;
            }
            fill_rect_color(
                surface,
                legend_x,
                legend_y + 3.0,
                6.0,
                6.0,
                chart_series_color(index),
            );
            let used = draw_chart_text(
                surface,
                &series.name,
                legend_x + 9.0,
                legend_y,
                (plot_right - legend_x - 9.0).max(1.0),
                ChartTextStyle {
                    size_pt: 8.0,
                    bold: false,
                    align: Alignment::Start,
                    color: Color::rgb(0x25, 0x2D, 0x36),
                },
                tcx,
            );
            legend_x += 9.0 + (series.name.chars().count() as f32 * 4.8).max(used * 3.0) + 12.0;
        }
        return;
    }

    if chart.kind == ChartKind::Waterfall {
        draw_waterfall_chart(surface, chart, plot_left, plot_top, plot_w, plot_h, tcx);
        return;
    }

    if chart.kind == ChartKind::Treemap {
        draw_treemap_chart(surface, chart, plot_left, plot_top, plot_w, plot_h, tcx);
        return;
    }

    if chart.kind == ChartKind::Sunburst {
        draw_sunburst_chart(surface, chart, plot_left, plot_top, plot_w, plot_h);
        return;
    }

    if chart.kind == ChartKind::BoxWhisker {
        draw_box_whisker_chart(surface, chart, plot_left, plot_top, plot_w, plot_h, tcx);
        return;
    }

    if chart.kind == ChartKind::Funnel {
        draw_funnel_chart(surface, chart, plot_left, plot_top, plot_w, plot_h, tcx);
        return;
    }

    let max_series_points = chart
        .series
        .iter()
        .map(|series| series.values.len())
        .max()
        .unwrap_or(0);
    let category_count = chart.categories.len().max(max_series_points).max(1);
    let series_count = chart.series.len().max(1);
    let (min_value, max_value) = if matches!(
        chart.kind,
        ChartKind::PercentStackedBar
            | ChartKind::PercentStackedBar3D
            | ChartKind::PercentStackedColumn
            | ChartKind::PercentStackedColumn3D
            | ChartKind::PercentStackedLine
            | ChartKind::PercentStackedArea
            | ChartKind::PercentStackedArea3D
    ) {
        (0.0, 1.0)
    } else if matches!(
        chart.kind,
        ChartKind::StackedBar
            | ChartKind::StackedBar3D
            | ChartKind::StackedColumn
            | ChartKind::StackedColumn3D
            | ChartKind::StackedLine
            | ChartKind::StackedArea
            | ChartKind::StackedArea3D
    ) {
        (0.0, stacked_chart_max(chart, category_count))
    } else {
        chart_value_range(chart)
    };
    let range = (max_value - min_value).max(1.0);
    let value_x = |value: f64| plot_left + (((value - min_value) / range) as f32 * plot_w);
    let value_y = |value: f64| plot_bottom - (((value - min_value) / range) as f32 * plot_h);

    match chart.kind {
        ChartKind::StackedBar
        | ChartKind::StackedBar3D
        | ChartKind::PercentStackedBar
        | ChartKind::PercentStackedBar3D => {
            let percent = matches!(
                chart.kind,
                ChartKind::PercentStackedBar | ChartKind::PercentStackedBar3D
            );
            for tick in 0..=4 {
                let frac = tick as f32 / 4.0;
                let x_tick = plot_left + frac * plot_w;
                fill_rect_color(surface, x_tick, plot_top, 0.35, plot_h, grid);
                let label = if percent {
                    format!("{}%", tick * 25)
                } else {
                    format_chart_tick(max_value * tick as f64 / 4.0)
                };
                draw_chart_text(
                    surface,
                    &label,
                    x_tick - 18.0,
                    plot_bottom + 3.0,
                    36.0,
                    ChartTextStyle {
                        size_pt: 7.5,
                        bold: false,
                        align: Alignment::Center,
                        color: Color::rgb(0x4C, 0x55, 0x5F),
                    },
                    tcx,
                );
            }
            fill_rect_color(surface, plot_left, plot_top, 0.8, plot_h, axis);
            fill_rect_color(surface, plot_left, plot_bottom, plot_w, 0.8, axis);

            let band_h = plot_h / category_count as f32;
            let bar_h = (band_h * 0.68).max(3.0);
            for (category_index, category) in chart.categories.iter().enumerate() {
                let band_top = plot_top + category_index as f32 * band_h;
                let label_y = band_top + (band_h - 9.0).max(0.0) * 0.5;
                draw_chart_text(
                    surface,
                    category,
                    x + 5.0,
                    label_y,
                    label_w,
                    ChartTextStyle {
                        size_pt: 8.0,
                        bold: false,
                        align: Alignment::End,
                        color: Color::rgb(0x25, 0x2D, 0x36),
                    },
                    tcx,
                );

                let bar_top = band_top + (band_h - bar_h) * 0.5;
                let mut offset = 0.0;
                let total = stacked_category_total(chart, category_index).max(1.0);
                for (series_index, series) in chart.series.iter().enumerate() {
                    let value = series
                        .values
                        .get(category_index)
                        .copied()
                        .filter(|value| value.is_finite() && *value > 0.0)
                        .unwrap_or(0.0);
                    if value <= 0.0 {
                        continue;
                    }
                    let start = if percent { offset / total } else { offset };
                    offset += value;
                    let end = if percent { offset / total } else { offset };
                    let segment_left = value_x(start).clamp(plot_left, plot_right);
                    let segment_right = value_x(end).clamp(plot_left, plot_right);
                    let color = chart_series_color(series_index);
                    if matches!(
                        chart.kind,
                        ChartKind::StackedBar3D | ChartKind::PercentStackedBar3D
                    ) {
                        fill_chart_bar_shape(
                            surface,
                            segment_left,
                            bar_top,
                            (segment_right - segment_left).max(1.0),
                            bar_h,
                            chart.shape,
                            color,
                        );
                    } else {
                        fill_rect_color(
                            surface,
                            segment_left,
                            bar_top,
                            (segment_right - segment_left).max(1.0),
                            bar_h,
                            color,
                        );
                    }
                }
            }
        }
        ChartKind::Bar | ChartKind::Bar3D => {
            let zero_x = value_x(0.0).clamp(plot_left, plot_right);
            for tick in 0..=4 {
                let frac = tick as f32 / 4.0;
                let x_tick = plot_left + frac * plot_w;
                fill_rect_color(surface, x_tick, plot_top, 0.35, plot_h, grid);
                let value = min_value + (max_value - min_value) * tick as f64 / 4.0;
                let label = format_chart_tick(value);
                draw_chart_text(
                    surface,
                    &label,
                    x_tick - 18.0,
                    plot_bottom + 3.0,
                    36.0,
                    ChartTextStyle {
                        size_pt: 7.5,
                        bold: false,
                        align: Alignment::Center,
                        color: Color::rgb(0x4C, 0x55, 0x5F),
                    },
                    tcx,
                );
            }
            fill_rect_color(surface, zero_x, plot_top, 0.8, plot_h, axis);
            fill_rect_color(surface, plot_left, plot_bottom, plot_w, 0.8, axis);

            let band_h = plot_h / category_count as f32;
            let group_h = (band_h * 0.68).max(3.0);
            let bar_h = ((group_h / series_count as f32) - 1.0).max(2.0);
            for (category_index, category) in chart.categories.iter().enumerate() {
                let band_top = plot_top + category_index as f32 * band_h;
                let label_y = band_top + (band_h - 9.0).max(0.0) * 0.5;
                draw_chart_text(
                    surface,
                    category,
                    x + 5.0,
                    label_y,
                    label_w,
                    ChartTextStyle {
                        size_pt: 8.0,
                        bold: false,
                        align: Alignment::End,
                        color: Color::rgb(0x25, 0x2D, 0x36),
                    },
                    tcx,
                );

                let group_top = band_top + (band_h - group_h) * 0.5;
                for (series_index, series) in chart.series.iter().enumerate() {
                    let value = series
                        .values
                        .get(category_index)
                        .copied()
                        .filter(|value| value.is_finite())
                        .unwrap_or(0.0);
                    let x_value = value_x(value).clamp(plot_left, plot_right);
                    let bar_left = zero_x.min(x_value);
                    let bar_width = (zero_x - x_value).abs().max(1.0);
                    let bar_top = group_top + series_index as f32 * (bar_h + 1.0);
                    let color = chart_series_color(series_index);
                    if chart.kind == ChartKind::Bar3D {
                        fill_chart_bar_shape(
                            surface,
                            bar_left,
                            bar_top,
                            bar_width,
                            bar_h,
                            chart.shape,
                            color,
                        );
                    } else {
                        fill_rect_color(surface, bar_left, bar_top, bar_width, bar_h, color);
                    }
                }
            }
        }
        ChartKind::Column
        | ChartKind::StackedColumn
        | ChartKind::PercentStackedColumn
        | ChartKind::Column3D
        | ChartKind::StackedColumn3D
        | ChartKind::PercentStackedColumn3D
        | ChartKind::Histogram
        | ChartKind::Line
        | ChartKind::LineNoMarkers
        | ChartKind::SmoothLine
        | ChartKind::StackedLine
        | ChartKind::PercentStackedLine
        | ChartKind::Line3D
        | ChartKind::Area
        | ChartKind::StackedArea
        | ChartKind::PercentStackedArea
        | ChartKind::Area3D
        | ChartKind::StackedArea3D
        | ChartKind::PercentStackedArea3D
        | ChartKind::Scatter
        | ChartKind::ScatterMarkers
        | ChartKind::ScatterLines
        | ChartKind::ScatterSmooth
        | ChartKind::ScatterSmoothNoMarkers
        | ChartKind::Bubble
        | ChartKind::Bubble3D
        | ChartKind::Surface
        | ChartKind::Surface3D
        | ChartKind::StockHighLowClose
        | ChartKind::Stock => {
            let zero_y = value_y(0.0).clamp(plot_top, plot_bottom);
            for tick in 0..=4 {
                let frac = tick as f32 / 4.0;
                let y_tick = plot_bottom - frac * plot_h;
                fill_rect_color(surface, plot_left, y_tick, plot_w, 0.35, grid);
                let value = min_value + (max_value - min_value) * tick as f64 / 4.0;
                let label = format_chart_tick(value);
                draw_chart_text(
                    surface,
                    &label,
                    x + 5.0,
                    y_tick - 5.0,
                    label_w,
                    ChartTextStyle {
                        size_pt: 7.5,
                        bold: false,
                        align: Alignment::End,
                        color: Color::rgb(0x4C, 0x55, 0x5F),
                    },
                    tcx,
                );
            }
            fill_rect_color(surface, plot_left, zero_y, plot_w, 0.8, axis);
            fill_rect_color(surface, plot_left, plot_top, 0.8, plot_h, axis);

            let band_w = plot_w / category_count as f32;
            for (category_index, category) in chart.categories.iter().enumerate() {
                let center_x = plot_left + category_index as f32 * band_w + band_w * 0.5;
                draw_chart_text(
                    surface,
                    category,
                    center_x - band_w * 0.48,
                    plot_bottom + 3.0,
                    band_w * 0.96,
                    ChartTextStyle {
                        size_pt: 8.0,
                        bold: false,
                        align: Alignment::Center,
                        color: Color::rgb(0x25, 0x2D, 0x36),
                    },
                    tcx,
                );
            }

            match chart.kind {
                ChartKind::StackedColumn
                | ChartKind::PercentStackedColumn
                | ChartKind::StackedColumn3D
                | ChartKind::PercentStackedColumn3D => {
                    let percent = matches!(
                        chart.kind,
                        ChartKind::PercentStackedColumn | ChartKind::PercentStackedColumn3D
                    );
                    let column_w = (band_w * 0.62).max(2.0);
                    for (category_index, _) in chart.categories.iter().enumerate() {
                        let column_left =
                            plot_left + category_index as f32 * band_w + (band_w - column_w) * 0.5;
                        let mut offset = 0.0;
                        let total = stacked_category_total(chart, category_index).max(1.0);
                        for (series_index, series) in chart.series.iter().enumerate() {
                            let value = series
                                .values
                                .get(category_index)
                                .copied()
                                .filter(|value| value.is_finite() && *value > 0.0)
                                .unwrap_or(0.0);
                            if value <= 0.0 {
                                continue;
                            }
                            let start = if percent { offset / total } else { offset };
                            offset += value;
                            let end = if percent { offset / total } else { offset };
                            let segment_bottom = value_y(start).clamp(plot_top, plot_bottom);
                            let segment_top = value_y(end).clamp(plot_top, plot_bottom);
                            let color = chart_series_color(series_index);
                            if matches!(
                                chart.kind,
                                ChartKind::StackedColumn3D | ChartKind::PercentStackedColumn3D
                            ) {
                                fill_chart_column_shape(
                                    surface,
                                    column_left,
                                    segment_top,
                                    column_w,
                                    (segment_bottom - segment_top).max(1.0),
                                    chart.shape,
                                    color,
                                );
                            } else {
                                fill_rect_color(
                                    surface,
                                    column_left,
                                    segment_top,
                                    column_w,
                                    (segment_bottom - segment_top).max(1.0),
                                    color,
                                );
                            }
                        }
                    }
                }
                ChartKind::Column | ChartKind::Column3D | ChartKind::Histogram => {
                    let group_w = (band_w * 0.68).max(3.0);
                    let column_w = ((group_w / series_count as f32) - 2.0).max(2.0);
                    for (category_index, _) in chart.categories.iter().enumerate() {
                        let group_left =
                            plot_left + category_index as f32 * band_w + (band_w - group_w) * 0.5;
                        for (series_index, series) in chart.series.iter().enumerate() {
                            let value = series
                                .values
                                .get(category_index)
                                .copied()
                                .filter(|value| value.is_finite())
                                .unwrap_or(0.0);
                            let y_value = value_y(value).clamp(plot_top, plot_bottom);
                            let column_top = zero_y.min(y_value);
                            let column_h = (zero_y - y_value).abs().max(1.0);
                            let column_left = group_left + series_index as f32 * (column_w + 2.0);
                            let color = chart_series_color(series_index);
                            if chart.kind == ChartKind::Column3D {
                                fill_chart_column_shape(
                                    surface,
                                    column_left,
                                    column_top,
                                    column_w,
                                    column_h,
                                    chart.shape,
                                    color,
                                );
                            } else {
                                fill_rect_color(
                                    surface,
                                    column_left,
                                    column_top,
                                    column_w,
                                    column_h,
                                    color,
                                );
                            }
                        }
                    }
                }
                ChartKind::Area
                | ChartKind::StackedArea
                | ChartKind::PercentStackedArea
                | ChartKind::Area3D
                | ChartKind::StackedArea3D
                | ChartKind::PercentStackedArea3D => {
                    if matches!(
                        chart.kind,
                        ChartKind::StackedArea
                            | ChartKind::PercentStackedArea
                            | ChartKind::StackedArea3D
                            | ChartKind::PercentStackedArea3D
                    ) {
                        let percent = matches!(
                            chart.kind,
                            ChartKind::PercentStackedArea | ChartKind::PercentStackedArea3D
                        );
                        for series_index in (0..chart.series.len()).rev() {
                            let color = chart_series_color(series_index);
                            let mut points = Vec::new();
                            for category_index in 0..chart.categories.len() {
                                let mut value = 0.0;
                                for series in chart.series.iter().take(series_index + 1) {
                                    value += series
                                        .values
                                        .get(category_index)
                                        .copied()
                                        .filter(|value| value.is_finite() && *value > 0.0)
                                        .unwrap_or(0.0);
                                }
                                if percent {
                                    value /= stacked_category_total(chart, category_index).max(1.0);
                                }
                                points.push((
                                    plot_left + category_index as f32 * band_w + band_w * 0.5,
                                    value_y(value).clamp(plot_top, plot_bottom),
                                ));
                            }
                            fill_area_shape(surface, &points, zero_y, color);
                        }
                    } else {
                        for (series_index, series) in chart.series.iter().enumerate() {
                            let color = chart_series_color(series_index);
                            let mut points = Vec::new();
                            for category_index in 0..chart.categories.len() {
                                let value = series
                                    .values
                                    .get(category_index)
                                    .copied()
                                    .filter(|value| value.is_finite())
                                    .unwrap_or(0.0);
                                points.push((
                                    plot_left + category_index as f32 * band_w + band_w * 0.5,
                                    value_y(value).clamp(plot_top, plot_bottom),
                                ));
                            }
                            fill_area_shape(surface, &points, zero_y, color);
                            let mut previous: Option<(f32, f32)> = None;
                            for (point_x, point_y) in points {
                                if let Some((prev_x, prev_y)) = previous {
                                    fill_line_segment(
                                        surface, prev_x, prev_y, point_x, point_y, 1.4, color,
                                    );
                                }
                                fill_rect_color(
                                    surface,
                                    point_x - 2.0,
                                    point_y - 2.0,
                                    4.0,
                                    4.0,
                                    color,
                                );
                                previous = Some((point_x, point_y));
                            }
                        }
                    }
                }
                ChartKind::Line
                | ChartKind::LineNoMarkers
                | ChartKind::SmoothLine
                | ChartKind::StackedLine
                | ChartKind::PercentStackedLine
                | ChartKind::Line3D => {
                    for (series_index, series) in chart.series.iter().enumerate() {
                        let color = chart_series_color(series_index);
                        let mut previous: Option<(f32, f32)> = None;
                        for category_index in 0..chart.categories.len() {
                            let value = if matches!(
                                chart.kind,
                                ChartKind::StackedLine | ChartKind::PercentStackedLine
                            ) {
                                let mut value = 0.0;
                                for series in chart.series.iter().take(series_index + 1) {
                                    value += series
                                        .values
                                        .get(category_index)
                                        .copied()
                                        .filter(|value| value.is_finite() && *value > 0.0)
                                        .unwrap_or(0.0);
                                }
                                if chart.kind == ChartKind::PercentStackedLine {
                                    value / stacked_category_total(chart, category_index).max(1.0)
                                } else {
                                    value
                                }
                            } else {
                                series
                                    .values
                                    .get(category_index)
                                    .copied()
                                    .filter(|value| value.is_finite())
                                    .unwrap_or(0.0)
                            };
                            let point_x = plot_left + category_index as f32 * band_w + band_w * 0.5;
                            let point_y = value_y(value).clamp(plot_top, plot_bottom);
                            if let Some((prev_x, prev_y)) = previous {
                                fill_line_segment(
                                    surface, prev_x, prev_y, point_x, point_y, 1.6, color,
                                );
                            }
                            if chart.kind != ChartKind::LineNoMarkers {
                                fill_rect_color(
                                    surface,
                                    point_x - 2.0,
                                    point_y - 2.0,
                                    4.0,
                                    4.0,
                                    color,
                                );
                            }
                            previous = Some((point_x, point_y));
                        }
                    }
                }
                ChartKind::StockHighLowClose | ChartKind::Stock => {
                    for category_index in 0..category_count {
                        let point_x = plot_left + category_index as f32 * band_w + band_w * 0.5;
                        let values: Vec<_> = chart
                            .series
                            .iter()
                            .filter_map(|series| {
                                series
                                    .values
                                    .get(category_index)
                                    .copied()
                                    .filter(|value| value.is_finite())
                            })
                            .collect();
                        if values.is_empty() {
                            continue;
                        }
                        let low = values.iter().copied().fold(f64::INFINITY, f64::min);
                        let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                        let y_low = value_y(low).clamp(plot_top, plot_bottom);
                        let y_high = value_y(high).clamp(plot_top, plot_bottom);
                        fill_rect_color(
                            surface,
                            point_x - 0.7,
                            y_high,
                            1.4,
                            (y_low - y_high).abs().max(1.0),
                            axis,
                        );
                        if chart.kind == ChartKind::Stock {
                            if let Some(open) = values.first().copied() {
                                let y_open = value_y(open).clamp(plot_top, plot_bottom);
                                fill_rect_color(
                                    surface,
                                    point_x - band_w * 0.18,
                                    y_open - 0.8,
                                    band_w * 0.18,
                                    1.6,
                                    chart_series_color(0),
                                );
                            }
                        }
                        if let Some(close) = values.last().copied() {
                            let color_index = if chart.kind == ChartKind::Stock {
                                3.min(chart.series.len().saturating_sub(1))
                            } else {
                                2.min(chart.series.len().saturating_sub(1))
                            };
                            let y_close = value_y(close).clamp(plot_top, plot_bottom);
                            fill_rect_color(
                                surface,
                                point_x,
                                y_close - 0.8,
                                band_w * 0.18,
                                1.6,
                                chart_series_color(color_index),
                            );
                        }
                    }
                }
                ChartKind::Scatter
                | ChartKind::ScatterMarkers
                | ChartKind::ScatterLines
                | ChartKind::ScatterSmooth
                | ChartKind::ScatterSmoothNoMarkers => {
                    for (series_index, series) in chart.series.iter().enumerate() {
                        let color = chart_series_color(series_index);
                        let mut previous: Option<(f32, f32)> = None;
                        for value_index in 0..series.values.len() {
                            let value = series
                                .values
                                .get(value_index)
                                .copied()
                                .filter(|value| value.is_finite())
                                .unwrap_or(0.0);
                            let point_x = plot_left + value_index as f32 * band_w + band_w * 0.5;
                            let point_y = value_y(value).clamp(plot_top, plot_bottom);
                            if chart.kind != ChartKind::ScatterMarkers {
                                if let Some((prev_x, prev_y)) = previous {
                                    fill_line_segment(
                                        surface, prev_x, prev_y, point_x, point_y, 1.3, color,
                                    );
                                }
                            }
                            if !matches!(
                                chart.kind,
                                ChartKind::ScatterLines | ChartKind::ScatterSmoothNoMarkers
                            ) {
                                fill_rect_color(
                                    surface,
                                    point_x - 2.5,
                                    point_y - 2.5,
                                    5.0,
                                    5.0,
                                    color,
                                );
                            }
                            previous = Some((point_x, point_y));
                        }
                    }
                }
                ChartKind::Bubble | ChartKind::Bubble3D => {
                    let (_, max_bubble_size) = chart_bubble_size_range(chart);
                    let max_radius = (band_w.min(plot_h) * 0.22).clamp(3.5, 14.0);
                    for (series_index, series) in chart.series.iter().enumerate() {
                        let color = chart_series_color(series_index);
                        for value_index in 0..series.values.len() {
                            let value = series
                                .values
                                .get(value_index)
                                .copied()
                                .filter(|value| value.is_finite())
                                .unwrap_or(0.0);
                            let size = series
                                .bubble_sizes
                                .get(value_index)
                                .copied()
                                .filter(|size| size.is_finite() && *size > 0.0)
                                .unwrap_or(1.0);
                            let point_x = plot_left + value_index as f32 * band_w + band_w * 0.5;
                            let point_y = value_y(value).clamp(plot_top, plot_bottom);
                            let radius = ((size / max_bubble_size).sqrt() as f32 * max_radius)
                                .clamp(2.5, max_radius);
                            fill_circle_color(surface, point_x, point_y, radius, color);
                        }
                    }
                }
                ChartKind::Surface | ChartKind::Surface3D => {
                    let row_count = chart.series.len().max(1);
                    let cell_h = (plot_h / row_count as f32).max(2.0);
                    for (series_index, series) in chart.series.iter().enumerate() {
                        let row_top = plot_top + series_index as f32 * cell_h;
                        draw_chart_text(
                            surface,
                            &series.name,
                            x + 5.0,
                            row_top + (cell_h - 8.0).max(0.0) * 0.5,
                            label_w,
                            ChartTextStyle {
                                size_pt: 7.5,
                                bold: false,
                                align: Alignment::End,
                                color: Color::rgb(0x25, 0x2D, 0x36),
                            },
                            tcx,
                        );
                        for category_index in 0..category_count {
                            let value = series
                                .values
                                .get(category_index)
                                .copied()
                                .filter(|value| value.is_finite())
                                .unwrap_or(0.0);
                            let intensity = ((value - min_value) / range).clamp(0.0, 1.0);
                            let shade = (0xEA as f64 - intensity * 0x70 as f64) as u8;
                            let color = rgb::Color::new(shade, (shade as f32 * 0.95) as u8, 0xF4);
                            let cell_left = plot_left + category_index as f32 * band_w + 1.0;
                            let cell_top = row_top + 1.0;
                            let cell_w = (band_w - 2.0).max(1.0);
                            let cell_h_inner = (cell_h - 2.0).max(1.0);
                            if chart.wireframe {
                                fill_rect_color(surface, cell_left, cell_top, cell_w, 0.45, color);
                                fill_rect_color(
                                    surface,
                                    cell_left,
                                    cell_top + cell_h_inner,
                                    cell_w,
                                    0.45,
                                    color,
                                );
                                fill_rect_color(
                                    surface,
                                    cell_left,
                                    cell_top,
                                    0.45,
                                    cell_h_inner,
                                    color,
                                );
                                fill_rect_color(
                                    surface,
                                    cell_left + cell_w,
                                    cell_top,
                                    0.45,
                                    cell_h_inner,
                                    color,
                                );
                            } else {
                                fill_rect_color(
                                    surface,
                                    cell_left,
                                    cell_top,
                                    cell_w,
                                    cell_h_inner,
                                    color,
                                );
                                fill_rect_color(surface, cell_left, cell_top, cell_w, 0.35, grid);
                            }
                        }
                    }
                }
                ChartKind::Bar
                | ChartKind::StackedBar
                | ChartKind::PercentStackedBar
                | ChartKind::Bar3D
                | ChartKind::StackedBar3D
                | ChartKind::PercentStackedBar3D
                | ChartKind::Radar
                | ChartKind::RadarWithMarkers
                | ChartKind::FilledRadar
                | ChartKind::Pie
                | ChartKind::ExplodedPie
                | ChartKind::Pie3D
                | ChartKind::ExplodedPie3D
                | ChartKind::PieOfPie
                | ChartKind::BarOfPie
                | ChartKind::Doughnut
                | ChartKind::ExplodedDoughnut
                | ChartKind::Waterfall
                | ChartKind::Treemap
                | ChartKind::Sunburst
                | ChartKind::BoxWhisker
                | ChartKind::Funnel => {}
            }
        }
        ChartKind::Radar
        | ChartKind::RadarWithMarkers
        | ChartKind::FilledRadar
        | ChartKind::Pie
        | ChartKind::ExplodedPie
        | ChartKind::Pie3D
        | ChartKind::ExplodedPie3D
        | ChartKind::PieOfPie
        | ChartKind::BarOfPie
        | ChartKind::Doughnut
        | ChartKind::ExplodedDoughnut
        | ChartKind::Waterfall
        | ChartKind::Treemap
        | ChartKind::Sunburst
        | ChartKind::BoxWhisker
        | ChartKind::Funnel => {}
    }

    let mut legend_x = plot_left;
    let legend_y = y + h - 14.0;
    for (index, series) in chart.series.iter().enumerate() {
        if legend_x >= plot_right - 20.0 {
            break;
        }
        fill_rect_color(
            surface,
            legend_x,
            legend_y + 3.0,
            6.0,
            6.0,
            chart_series_color(index),
        );
        let used = draw_chart_text(
            surface,
            &series.name,
            legend_x + 9.0,
            legend_y,
            (plot_right - legend_x - 9.0).max(1.0),
            ChartTextStyle {
                size_pt: 8.0,
                bold: false,
                align: Alignment::Start,
                color: Color::rgb(0x25, 0x2D, 0x36),
            },
            tcx,
        );
        legend_x += 9.0 + (series.name.chars().count() as f32 * 4.8).max(used * 3.0) + 12.0;
    }
}

/// Draw a run's glyphs at an absolute baseline position, in the run's color.
fn draw_run(surface: &mut Surface<'_>, run: RunDraw, x_abs: f32, baseline_y: f32) {
    let x = x_abs + run.x;
    let baseline = baseline_y + run.baseline_shift;
    let width = run.width();
    if let Some(highlight) = run.highlight {
        fill_rect_color(
            surface,
            x,
            baseline - run.ascent,
            width,
            run.ascent + run.descent,
            highlight,
        );
    }
    surface.set_fill(Some(Fill {
        paint: run.color.into(),
        rule: FillRule::NonZero,
        opacity: NormalizedF32::ONE,
    }));
    surface.draw_glyphs(
        Point::from_xy(x, baseline),
        &run.glyphs,
        run.font,
        &run.text,
        run.size,
        false,
    );
    if let Some(decoration) = run.underline {
        fill_rect_color(
            surface,
            x,
            baseline + decoration.offset,
            width,
            decoration.thickness,
            run.color,
        );
    }
    if let Some(decoration) = run.strikethrough {
        fill_rect_color(
            surface,
            x,
            baseline + decoration.offset,
            width,
            decoration.thickness,
            run.color,
        );
    }
}

fn draw_run_with_page_context(
    surface: &mut Surface<'_>,
    run: RunDraw,
    x_abs: f32,
    baseline_y: f32,
    page_number: usize,
    tcx: &mut TextCx<'_>,
) {
    let Some(dynamic) = run.dynamic.clone() else {
        draw_run(surface, run, x_abs, baseline_y);
        return;
    };

    let text = match dynamic.kind {
        DynamicTextKind::PageNumber => page_number.to_string(),
    };
    let Some(line) = shape(
        &text,
        StyledText::plain(&[(0, text.len(), dynamic.props)]),
        None,
        Alignment::Start,
        1024.0,
        tcx,
    )
    .into_iter()
    .next() else {
        draw_run(surface, run, x_abs, baseline_y);
        return;
    };

    for replacement in line.runs {
        draw_run(surface, replacement, x_abs + run.x, baseline_y);
    }
}

struct PlacedItem {
    x: f32,
    width: f32,
    top: f32,
    item: FlowItem,
}

type Pages = Vec<Vec<PlacedItem>>;

#[derive(Debug, Clone, Copy)]
struct ColumnLayout {
    count: usize,
    width: f32,
}

impl ColumnLayout {
    fn new(geom: Geom, requested: Option<u16>) -> Self {
        let content_width = geom.content_w();
        let max_by_width = ((content_width + COLUMN_GAP_PT) / (MIN_COLUMN_WIDTH_PT + COLUMN_GAP_PT))
            .floor()
            .max(1.0) as usize;
        let count = usize::from(requested.unwrap_or(1).max(1))
            .min(MAX_SECTION_COLUMNS)
            .min(max_by_width);
        let gaps = COLUMN_GAP_PT * count.saturating_sub(1) as f32;
        Self {
            count,
            width: ((content_width - gaps) / count as f32).max(MIN_COLUMN_WIDTH_PT),
        }
    }

    fn x(self, index: usize) -> f32 {
        index.min(self.count.saturating_sub(1)) as f32 * (self.width + COLUMN_GAP_PT)
    }
}

struct FlowCursor {
    columns: ColumnLayout,
    column_index: usize,
    y: f32,
    column_nonempty: bool,
}

impl FlowCursor {
    fn new(geom: Geom, columns: Option<u16>) -> Self {
        Self {
            columns: ColumnLayout::new(geom, columns),
            column_index: 0,
            y: geom.top(),
            column_nonempty: false,
        }
    }

    fn set_columns(&mut self, geom: Geom, columns: Option<u16>) {
        self.columns = ColumnLayout::new(geom, columns);
        self.column_index = 0;
        self.y = geom.top();
        self.column_nonempty = false;
    }

    fn advance(&mut self, pages: &mut Pages, geom: Geom) {
        if self.column_index + 1 < self.columns.count {
            self.column_index += 1;
        } else {
            pages.push(Vec::new());
            self.column_index = 0;
        }
        self.y = geom.top();
        self.column_nonempty = false;
    }

    fn force_page(&mut self, pages: &mut Pages, geom: Geom) {
        pages.push(Vec::new());
        self.column_index = 0;
        self.y = geom.top();
        self.column_nonempty = false;
    }
}

/// Layout-derived page map from rwml's preview-grade pagination.
///
/// This matches rwml's own PDF output, not Microsoft Word's pagination. Page
/// indices are physical, 1-based page numbers; section page-number restarts and
/// formats are intentionally not applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutPages {
    /// Total number of physical pages produced by rwml's preview paginator.
    pub pages: usize,
    /// First physical page each top-level body block touches, in model order.
    pub block_pages: Vec<Option<usize>>,
    /// Physical page for each body `PAGE` field occurrence, in model order.
    pub page_fields: Vec<Option<usize>>,
}

struct PdfRender {
    pdf: Vec<u8>,
    pages: usize,
}

struct Pagination {
    pages: Pages,
    page_sections: Vec<Option<RenderPageSection>>,
    block_pages: HashMap<usize, usize>,
    block_line_pages: HashMap<usize, Vec<BlockLinePage>>,
    final_section_start_page_index: usize,
}

#[derive(Clone, Copy)]
struct ActiveTopBottomBand {
    owner_block: Option<usize>,
    page_index: usize,
    top: f32,
    bottom: f32,
}

#[derive(Clone, Copy)]
struct PendingTopBottomBand {
    owner_block: Option<usize>,
    anchor_offset: usize,
    top: f32,
    bottom: f32,
}

/// Place an item at the current `y` on the last page, then advance `y`.
fn place_item(pages: &mut Pages, cursor: &mut FlowCursor, item: FlowItem, h: f32) {
    if let Some(p) = pages.last_mut() {
        p.push(PlacedItem {
            x: cursor.columns.x(cursor.column_index),
            width: cursor.columns.width,
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

fn top_bottom_adjusted_y(
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

fn first_row_fragment_height(row: &RowLayout) -> f32 {
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
    // Only repeat headers that fit a page. A header taller than the content box would overflow
    // on every page (place_item does not split it), forcing each following body row to break to
    // a fresh page and re-clone the whole header — O(rows × header_lines). Dropping the repeat
    // for an over-tall header keeps pagination linear (the header still renders inline once).
    let page_h = geom.bottom() - geom.top();
    if headers.iter().map(|h| h.height).sum::<f32>() > page_h {
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
        let next_start = starts.get(position + 1).copied().unwrap_or(items.len());
        let end = items[start + 1..next_start]
            .iter()
            .position(|item| matches!(item, FlowItem::PaginationBoundary))
            .map(|offset| start + 1 + offset)
            .unwrap_or(next_start);
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
                | FlowItem::SectionBreak(_)
                | FlowItem::Table { .. }
                | FlowItem::Picture { .. }
                | FlowItem::Chart { .. } => is_paragraph = false,
            }
        }
        is_paragraph &= !line_heights.is_empty();
        metrics[start] = Some(BlockPaginationMetrics {
            pagination,
            next_start: starts.get(position + 1).copied(),
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

fn paginate(items: Vec<FlowItem>, geom: Geom, final_section_setup: &SectionSetup) -> Pagination {
    // Paginate: flow items top-to-bottom through equal-width columns and then
    // across pages. Tables repeat headers after each break and split oversized rows.
    let columns_by_item = section_columns_by_item(&items, final_section_setup.columns);
    let block_metrics = block_pagination_metrics(&items);
    let mut pages: Pages = vec![Vec::new()];
    let mut page_sections: Vec<Option<RenderPageSection>> = vec![None];
    let mut section_start_page_index = 0usize;
    let mut active_columns = columns_by_item
        .first()
        .copied()
        .unwrap_or(final_section_setup.columns);
    let mut cursor = FlowCursor::new(geom, active_columns);
    let mut block_pages = HashMap::new();
    let mut block_line_pages: HashMap<usize, Vec<BlockLinePage>> = HashMap::new();
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
        let item_columns = columns_by_item[item_index];
        if item_columns != active_columns {
            cursor.set_columns(geom, item_columns);
            active_columns = item_columns;
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
                                geom,
                                &active_top_bottom_bands,
                            );
                        }
                    }
                    let keep_whole_paragraph = pagination.keep_lines
                        || (pagination.widow_control
                            && metric.line_heights.len() <= 3
                            && metric.last_line_extent <= geom.bottom() - geom.top());
                    if keep_whole_paragraph {
                        move_to_fresh_column_for_required_height(
                            &mut pages,
                            &mut cursor,
                            metric.last_line_extent,
                            geom,
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
                        top: top.max(geom.top()),
                        bottom: bottom.min(geom.bottom()),
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
                    geom,
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
                            cursor.advance(&mut pages, geom);
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
                                geom,
                                &active_top_bottom_bands,
                            );
                            if fits < remaining {
                                if fits < 2 && cursor.column_nonempty {
                                    cursor.advance(&mut pages, geom);
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
                                            && remaining_height <= geom.bottom() - geom.top()
                                        {
                                            cursor.advance(&mut pages, geom);
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
                    geom,
                    &active_top_bottom_bands,
                    None,
                );
                let page_index = pages.len().saturating_sub(1);
                record_pending_block_page(&mut block_pages, &mut pending_block, page_index);
                record_block_line_page(&mut block_line_pages, current_block, &l, page_index);
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
                    geom,
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
                    geom,
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
                let first_page = place_table(&mut pages, &mut cursor, rows, header_rows, geom)
                    .unwrap_or(fallback_page);
                record_pending_block_page(&mut block_pages, &mut pending_block, first_page);
            }
            FlowItem::PageBreak => {
                cursor.force_page(&mut pages, geom);
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
            }
            FlowItem::SectionBreak(section) => {
                let next_section_page =
                    page_after_section_break(pages.len(), section.section_break);
                while pages.len() < next_section_page {
                    cursor.force_page(&mut pages, geom);
                }
                page_sections.resize(pages.len(), None);
                assign_section_to_render_pages(
                    &mut page_sections,
                    section_start_page_index,
                    next_section_page.saturating_sub(2),
                    &section,
                );
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                section_start_page_index = next_section_page.saturating_sub(1);
            }
            // Rows reach pagination only inside a Table; place defensively.
            FlowItem::Row(r) => {
                let h = r.height;
                ensure_outside_top_bottom_bands(
                    &mut pages,
                    &mut cursor,
                    h,
                    geom,
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
    );
    Pagination {
        pages,
        page_sections,
        block_pages,
        block_line_pages,
        final_section_start_page_index: section_start_page_index,
    }
}

fn collect_pdf_flow_items(
    model: &DocModel,
    geom: Geom,
    tcx: &mut TextCx<'_>,
    capture: &mut LayoutCapture,
    source_hints: SourceRenderHints<'_>,
    floating_shapes: &[FloatingShape],
    unsupported_features: Option<&FeatureInventory>,
) -> Vec<FlowItem> {
    let mut items: Vec<FlowItem> = Vec::new();
    let final_section_setup = SectionSetup::from(&model.setup);
    let body_columns = section_columns_by_block(&model.blocks, final_section_setup.columns);
    let top_bottom_bands = top_bottom_bands_by_block(model, floating_shapes, geom);
    collect_blocks_with_block_anchors(
        &model.blocks,
        &mut items,
        geom,
        tcx,
        capture,
        BodyCollectionSidecars {
            section_columns: &body_columns,
            pagination_hints: source_hints.pagination,
            tab_stops: source_hints.tab_stops,
            default_tab_stop_pt: source_hints.default_tab_stop_pt,
            table_row_pagination: source_hints.table_row_pagination,
            table_cell_pagination: source_hints.table_cell_pagination,
            table_nested_pagination: source_hints.table_nested_pagination,
            table_cell_tab_stops: source_hints.table_cell_tab_stops,
            top_bottom_bands: &top_bottom_bands,
        },
    );
    items.push(FlowItem::PaginationBoundary);
    let final_column_geom =
        geom.with_content_width(ColumnLayout::new(geom, final_section_setup.columns).width);
    if let Some(features) = unsupported_features {
        let placeholders = unsupported_placeholder_blocks(
            features,
            floating_shapes.len().min(MAX_FLOATING_SHAPE_OVERLAYS),
        );
        if !placeholders.is_empty() {
            if !items.is_empty() {
                items.push(FlowItem::Gap(PARA_GAP));
            }
            collect_blocks(&placeholders, &mut items, final_column_geom, tcx, capture);
        }
    }
    let missing_image_placeholders =
        missing_image_placeholder_blocks(count_missing_image_bytes(&model.blocks));
    if !missing_image_placeholders.is_empty() {
        if !items.is_empty() {
            items.push(FlowItem::Gap(PARA_GAP));
        }
        collect_blocks(
            &missing_image_placeholders,
            &mut items,
            final_column_geom,
            tcx,
            capture,
        );
    }
    let undecodable_placeholders =
        undecodable_image_placeholder_blocks(count_undecodable_images(&model.blocks));
    if !undecodable_placeholders.is_empty() {
        if !items.is_empty() {
            items.push(FlowItem::Gap(PARA_GAP));
        }
        collect_blocks(
            &undecodable_placeholders,
            &mut items,
            final_column_geom,
            tcx,
            capture,
        );
    }
    items
}

fn section_columns_by_block(blocks: &[Block], final_columns: Option<u16>) -> Vec<Option<u16>> {
    let mut columns = vec![final_columns; blocks.len()];
    let mut section_start = 0usize;
    for (index, block) in blocks.iter().enumerate() {
        if let Block::SectionBreak(setup) = block {
            columns[section_start..=index].fill(setup.columns);
            section_start = index + 1;
        }
    }
    columns
}

fn strict_font_context(fonts: &[Vec<u8>]) -> Result<FontContext> {
    use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};

    if fonts.is_empty() {
        return Err(Error::Render(
            "layout page calculation requires at least one font".to_string(),
        ));
    }

    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    let mut registered = 0usize;
    for font in fonts {
        if font.is_empty() {
            continue;
        }
        registered += collection
            .register_fonts(Blob::from(font.clone()), None)
            .into_iter()
            .map(|(_, fonts)| fonts.len())
            .sum::<usize>();
    }
    if registered == 0 {
        return Err(Error::Render(
            "layout page calculation could not register any supplied fonts".to_string(),
        ));
    }

    Ok(FontContext {
        collection,
        source_cache: SourceCache::default(),
    })
}

fn record_line_page_fields(
    line: &LineLayout,
    page_number: usize,
    page_fields: &mut [Option<usize>],
) {
    for run in &line.runs {
        let Some(index) = run
            .dynamic
            .as_ref()
            .and_then(|dynamic| dynamic.page_field_index)
        else {
            continue;
        };
        if let Some(slot) = page_fields.get_mut(index) {
            if slot.is_none() {
                *slot = Some(page_number);
            }
        }
    }
}

fn record_page_fields(pages: &Pages, page_fields: &mut [Option<usize>]) {
    for (page_index, page_items) in pages.iter().enumerate() {
        let page_number = page_index + 1;
        for placed in page_items {
            match &placed.item {
                FlowItem::Line(line) => record_line_page_fields(line, page_number, page_fields),
                FlowItem::Row(row) => {
                    for cell in &row.cells {
                        for line in &cell.lines {
                            record_line_page_fields(line, page_number, page_fields);
                        }
                    }
                }
                FlowItem::BlockStart { .. }
                | FlowItem::TopBottomBand { .. }
                | FlowItem::PaginationBoundary
                | FlowItem::Gap(_)
                | FlowItem::PageBreak
                | FlowItem::SectionBreak(_)
                | FlowItem::Table { .. }
                | FlowItem::Picture { .. }
                | FlowItem::Chart { .. } => {}
            }
        }
    }
}

/// Return layout-derived page numbers from rwml's preview-grade pagination.
///
/// This matches rwml's own PDF output, not Microsoft Word's pagination. Page
/// indices are physical, 1-based page numbers; section page-number restarts and
/// formats are intentionally not applied. The supplied fonts are used strictly:
/// system fonts are disabled and only successfully registered caller bytes are
/// considered.
pub fn layout_pages_with_fonts(model: &DocModel, fonts: &[Vec<u8>]) -> Result<LayoutPages> {
    layout_pages_with_fonts_and_pagination(model, fonts, SourceRenderHints::default(), &[])
}

pub(crate) fn layout_pages_with_fonts_and_pagination(
    model: &DocModel,
    fonts: &[Vec<u8>],
    source_hints: SourceRenderHints<'_>,
    floating_shapes: &[FloatingShape],
) -> Result<LayoutPages> {
    let mut font_cx = strict_font_context(fonts)?;
    let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
    let mut font_cache: HashMap<u64, Font> = HashMap::new();
    let mut tcx = TextCx {
        font_cx: &mut font_cx,
        layout_cx: &mut layout_cx,
        font_cache: &mut font_cache,
    };
    let geom = Geom::from_setup(&model.setup.page);
    let mut capture = LayoutCapture::page_fields();
    let items = collect_pdf_flow_items(
        model,
        geom,
        &mut tcx,
        &mut capture,
        source_hints,
        floating_shapes,
        None,
    );
    let final_section_setup = SectionSetup::from(&model.setup);
    let pagination = paginate(items, geom, &final_section_setup);
    let mut page_fields = capture.page_fields;
    record_page_fields(&pagination.pages, &mut page_fields);
    let block_pages = (0..model.blocks.len())
        .map(|index| pagination.block_pages.get(&index).map(|page| page + 1))
        .collect();

    Ok(LayoutPages {
        pages: pagination.pages.len(),
        block_pages,
        page_fields,
    })
}

/// Render a [`DocModel`] to PDF using system fonts.
pub(crate) fn to_pdf(model: &DocModel) -> Vec<u8> {
    to_pdf_with_fonts(model, &[])
}

/// Fallible variant of [`to_pdf`].
pub(crate) fn try_to_pdf(model: &DocModel) -> Result<Vec<u8>> {
    try_to_pdf_with_fonts(model, &[])
}

/// Render a [`DocModel`] to PDF, first registering each blob in `extra_fonts` into
/// the layout font collection. Lets a caller supply a Korean (or any) font so
/// rendering works in environments without matching system fonts — the font is
/// then available by its own family name and participates in script fallback.
/// Undecodable font blobs are ignored.
pub(crate) fn to_pdf_with_fonts(model: &DocModel, extra_fonts: &[Vec<u8>]) -> Vec<u8> {
    try_to_pdf_with_fonts(model, extra_fonts).unwrap_or_default()
}

/// Fallible variant of [`to_pdf_with_fonts`].
pub(crate) fn try_to_pdf_with_fonts(model: &DocModel, extra_fonts: &[Vec<u8>]) -> Result<Vec<u8>> {
    Ok(render_pdf(model, extra_fonts, None, &[], SourceRenderHints::default())?.pdf)
}

pub(crate) fn to_pdf_with_fonts_and_features_and_shapes(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
    floating_shapes: &[FloatingShape],
    source_hints: SourceRenderHints<'_>,
) -> Vec<u8> {
    try_to_pdf_with_fonts_and_features_and_shapes(
        model,
        extra_fonts,
        features,
        floating_shapes,
        source_hints,
    )
    .unwrap_or_default()
}

pub(crate) fn try_to_pdf_with_fonts_and_features_and_shapes(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
    floating_shapes: &[FloatingShape],
    source_hints: SourceRenderHints<'_>,
) -> Result<Vec<u8>> {
    let unsupported = report::render_unsupported_features(&features);
    Ok(render_pdf(
        model,
        extra_fonts,
        Some(&unsupported),
        floating_shapes,
        source_hints,
    )?
    .pdf)
}

pub(crate) fn to_pdf_with_fonts_and_report(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
) -> RenderedPdf {
    to_pdf_with_fonts_and_report_and_shapes(
        model,
        extra_fonts,
        features,
        &[],
        SourceRenderHints::default(),
    )
}

pub(crate) fn to_pdf_with_fonts_and_report_and_shapes(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
    floating_shapes: &[FloatingShape],
    source_hints: SourceRenderHints<'_>,
) -> RenderedPdf {
    let unsupported = report::render_unsupported_features(&features);
    let fallback_unsupported = unsupported.clone();
    try_to_pdf_with_fonts_and_report_and_shapes(
        model,
        extra_fonts,
        features,
        floating_shapes,
        source_hints,
    )
    .unwrap_or_else(|_| RenderedPdf {
        pdf: Vec::new(),
        report: RenderReport {
            pages: 0,
            warnings: render_warnings_for_model(&fallback_unsupported, model),
            unsupported: fallback_unsupported,
        },
    })
}

pub(crate) fn try_to_pdf_with_fonts_and_report(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
) -> Result<RenderedPdf> {
    try_to_pdf_with_fonts_and_report_and_shapes(
        model,
        extra_fonts,
        features,
        &[],
        SourceRenderHints::default(),
    )
}

pub(crate) fn try_to_pdf_with_fonts_and_report_and_shapes(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
    floating_shapes: &[FloatingShape],
    source_hints: SourceRenderHints<'_>,
) -> Result<RenderedPdf> {
    let unsupported = report::render_unsupported_features(&features);
    let rendered = render_pdf(
        model,
        extra_fonts,
        Some(&unsupported),
        floating_shapes,
        source_hints,
    )?;
    let warnings = render_warnings_for_model(&unsupported, model);
    Ok(RenderedPdf {
        pdf: rendered.pdf,
        report: RenderReport {
            pages: rendered.pages,
            warnings,
            unsupported,
        },
    })
}

fn render_pdf(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    unsupported_features: Option<&FeatureInventory>,
    floating_shapes: &[FloatingShape],
    source_hints: SourceRenderHints<'_>,
) -> Result<PdfRender> {
    use parley::fontique::Blob;
    let mut font_cx = FontContext::default();
    for f in extra_fonts {
        if !f.is_empty() {
            font_cx
                .collection
                .register_fonts(Blob::from(f.clone()), None);
        }
    }
    let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
    let mut font_cache: HashMap<u64, Font> = HashMap::new();
    let mut tcx = TextCx {
        font_cx: &mut font_cx,
        layout_cx: &mut layout_cx,
        font_cache: &mut font_cache,
    };
    // Page geometry from the document (Letter/A4/A3/landscape/custom margins).
    let geom = Geom::from_setup(&model.setup.page);
    let mut capture = LayoutCapture::default();
    let items = collect_pdf_flow_items(
        model,
        geom,
        &mut tcx,
        &mut capture,
        source_hints,
        floating_shapes,
        unsupported_features,
    );
    let final_section_setup = SectionSetup::from(&model.setup);
    let pagination = paginate(items, geom, &final_section_setup);
    let pages = pagination.pages;
    let page_sections = pagination.page_sections;
    let section_start_page_index = pagination.final_section_start_page_index;
    let floating_shape_overlays = floating_shape_overlays_for_pages(
        floating_shapes,
        geom,
        &pagination.block_pages,
        &pagination.block_line_pages,
    );

    // Emit.
    let mut document = PdfDoc::new();
    let page_count = pages.len();
    for (page_index, page_items) in pages.into_iter().enumerate() {
        let Some(settings) = PageSettings::from_wh(geom.page_w, geom.page_h) else {
            continue;
        };
        let mut page = document.start_page_with(settings);
        // Link rects collected while drawing (top-down coords); added as annotations
        // after the surface is finished (which releases its borrow on the page).
        let mut page_links: Vec<(f32, f32, f32, f32, Rc<str>)> = Vec::new();
        let page_number = page_index + 1;
        let fallback_page_section;
        let page_section = match page_sections.get(page_index).and_then(Option::as_ref) {
            Some(section) => section,
            None => {
                fallback_page_section = RenderPageSection {
                    setup: final_section_setup.clone(),
                    first_page_index: section_start_page_index,
                };
                &fallback_page_section
            }
        };
        let (header_blocks, footer_blocks) = running_header_footer_blocks_for_page(
            &page_section.setup,
            page_number,
            page_index == page_section.first_page_index,
        );
        let header_lines = layout_lines(header_blocks, geom, &mut tcx);
        let footer_lines = layout_lines(footer_blocks, geom, &mut tcx);
        let mut surface = page.surface();
        for overlay in floating_shape_overlays
            .iter()
            .filter(|overlay| overlay.page_index == page_index && overlay.behind_doc)
        {
            draw_floating_shape_overlay(&mut surface, overlay, &mut tcx);
        }
        // Running header (top margin) and footer (below the content box), on every
        // page. Lines are cloned because drawing consumes the glyph runs.
        let mut hy = HEADER_Y;
        for line in &header_lines {
            // Clamp to the top margin so a tall/multi-line header can't bleed into
            // the body content area.
            if hy + line.height > geom.top() {
                break;
            }
            let baseline = hy + line.baseline;
            let x0 = geom.left + line.x_indent;
            draw_line_background(&mut surface, line, x0, hy);
            for run in &line.runs {
                draw_run_with_page_context(
                    &mut surface,
                    run.clone(),
                    x0,
                    baseline,
                    page_index + 1,
                    &mut tcx,
                );
            }
            hy += line.height;
        }
        let mut fy = geom.bottom() + FOOTER_GAP;
        for line in &footer_lines {
            // Clamp to the page so a tall footer doesn't run off the bottom edge.
            if fy + line.height > geom.page_h {
                break;
            }
            let baseline = fy + line.baseline;
            let x0 = geom.left + line.x_indent;
            draw_line_background(&mut surface, line, x0, fy);
            for run in &line.runs {
                draw_run_with_page_context(
                    &mut surface,
                    run.clone(),
                    x0,
                    baseline,
                    page_index + 1,
                    &mut tcx,
                );
            }
            fy += line.height;
        }
        if page_section.setup.page_numbers {
            if let Some(line) = layout_page_number_line(page_index + 1, geom, &mut tcx) {
                if fy + line.height <= geom.page_h {
                    let baseline = fy + line.baseline;
                    let x0 = geom.left + line.x_indent;
                    draw_line_background(&mut surface, &line, x0, fy);
                    for run in line.runs {
                        draw_run(&mut surface, run, x0, baseline);
                    }
                }
            }
        }
        let mut previous_row_borders: Option<RenderedRowBorders> = None;
        for placed in page_items {
            let top = placed.top;
            let column_x = placed.x;
            match placed.item {
                FlowItem::BlockStart { .. }
                | FlowItem::TopBottomBand { .. }
                | FlowItem::PaginationBoundary
                | FlowItem::Gap(_)
                | FlowItem::PageBreak
                | FlowItem::SectionBreak(_)
                | FlowItem::Table { .. } => {}
                FlowItem::Picture { image, layout } => {
                    // Center the rotated visual bounds within the active body column.
                    let bounds_x =
                        geom.left + column_x + ((placed.width - layout.bounds_w) * 0.5).max(0.0);
                    if let Some(sz) = Size::from_wh(layout.image_w, layout.image_h) {
                        surface.push_transform(&image_paint_transform(layout, bounds_x, top));
                        surface.draw_image(image, sz);
                        surface.pop();
                    }
                }
                FlowItem::Chart { chart, w, h } => {
                    let x = geom.left + column_x + ((placed.width - w) * 0.5).max(0.0);
                    draw_authored_chart(&mut surface, &chart, x, top, w, h, &mut tcx);
                }
                FlowItem::Line(line) => {
                    let baseline = top + line.baseline;
                    let x0 = geom.left + column_x + line.x_indent;
                    let lh = line.height;
                    draw_line_background(&mut surface, &line, x0, top);
                    for run in line.runs {
                        if let Some(url) = run.link.clone() {
                            let l = x0 + run.x;
                            page_links.push((l, top, l + run.width(), top + lh, url));
                        }
                        draw_run_with_page_context(
                            &mut surface,
                            run,
                            x0,
                            baseline,
                            page_index + 1,
                            &mut tcx,
                        );
                    }
                }
                FlowItem::Row(row) => {
                    let table_id = row.table_id;
                    let border = row.border;
                    let row_height = row.height;
                    let bottom = top + row_height;
                    let x_offset = geom.left + column_x;
                    let current_vertical = row_vertical_border_lines(&row, x_offset);
                    let junctions = match (table_id, previous_row_borders.as_ref()) {
                        (Some(table_id), Some(previous))
                            if previous.table_id == table_id
                                && (previous.bottom - top).abs() < 0.01 =>
                        {
                            terminal_vertical_junctions(
                                &previous.vertical,
                                &row,
                                &current_vertical,
                                x_offset,
                            )
                        }
                        _ => Vec::new(),
                    };
                    let cells = row.cells;
                    if junctions.is_empty() {
                        for cell in cells {
                            draw_table_cell_background_and_borders(
                                &mut surface,
                                &cell,
                                x_offset,
                                top,
                                bottom,
                                row_height,
                                border,
                            );
                            let cell_x = x_offset + cell.x;
                            draw_table_cell_content(
                                &mut surface,
                                cell,
                                CellContentPlacement {
                                    x: cell_x,
                                    top,
                                    row_height,
                                },
                                page_number,
                                &mut tcx,
                                &mut page_links,
                            );
                        }
                    } else {
                        for cell in &cells {
                            draw_table_cell_background_and_borders(
                                &mut surface,
                                cell,
                                x_offset,
                                top,
                                bottom,
                                row_height,
                                border,
                            );
                        }
                        for (line, horizontal_width) in junctions {
                            draw_terminal_vertical_junction(
                                &mut surface,
                                top,
                                line,
                                horizontal_width,
                            );
                        }
                        for cell in cells {
                            let cell_x = x_offset + cell.x;
                            draw_table_cell_content(
                                &mut surface,
                                cell,
                                CellContentPlacement {
                                    x: cell_x,
                                    top,
                                    row_height,
                                },
                                page_number,
                                &mut tcx,
                                &mut page_links,
                            );
                        }
                    }
                    previous_row_borders = table_id.map(|table_id| RenderedRowBorders {
                        table_id,
                        bottom,
                        vertical: current_vertical,
                    });
                }
            }
        }
        for overlay in floating_shape_overlays
            .iter()
            .filter(|overlay| overlay.page_index == page_index && !overlay.behind_doc)
        {
            draw_floating_shape_overlay(&mut surface, overlay, &mut tcx);
        }
        surface.finish();
        for (l, t, r, b, url) in page_links {
            if let Some(rect) = Rect::from_ltrb(l, t, r, b) {
                let target = Target::Action(LinkAction::new(url.to_string()).into());
                page.add_annotation(Annotation::new_link(
                    LinkAnnotation::new(rect, target),
                    None,
                ));
            }
        }
        page.finish();
    }
    let pdf = document
        .finish()
        .map_err(|e| Error::Render(e.to_string()))?;
    Ok(PdfRender {
        pdf,
        pages: page_count,
    })
}

#[cfg(test)]
mod tests {
    use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
    use parley::{FontContext, LayoutContext};
    use std::collections::HashMap;

    use super::{
        assign_section_to_render_pages, cell_insets, cell_line_origin, count_missing_image_bytes,
        display_text, first_row_fragment_height, image_layout, image_paint_transform,
        layout_page_number_line, layout_paragraph, layout_table, layout_table_with_row_pagination,
        page_field_text, paginate, rgb, running_header_footer_blocks_for_page, shape, shape_cell,
        split_row, unsupported_placeholder_texts, FlowItem, Geom, LayoutCapture, LineLayout,
        StyledText, TablePaginationView, TextCx, DEFAULT_TAB_STOP_PT,
    };
    use crate::model::{
        Align, Block, Cell, CellMargins, CharProps, Color, DocModel, FieldRole, Image, Indent,
        ListInfo, PageSetup, PaginationHint, ParaProps, Paragraph, Row, Run, SectionBreakKind,
        SectionSetup, Spacing, TabAlignment, TabStop, Table, TableBorderSide, TablePaginationHints,
        TableRowPaginationHint, VCell, VertAlign,
    };
    use crate::report::FeatureInventory;
    use crate::{FloatingShape, ShapeEffectExtent, ShapeExtent, ShapePoint, ShapePosition};

    fn strict_font_context(fonts: &[Vec<u8>]) -> FontContext {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        for font in fonts {
            collection.register_fonts(Blob::from(font.clone()), None);
        }
        FontContext {
            collection,
            source_cache: SourceCache::default(),
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}"
        );
    }

    /// Pins the parley contract the tab-aware breaking path relies on: driving
    /// `break_lines` incrementally lets the caller narrow an individual line's
    /// max advance, which moves trailing content onto the next line without
    /// touching the shaped layout.
    #[test]
    fn per_line_max_advance_moves_trailing_content() {
        use parley::style::StyleProperty;

        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();

        let text = "가나 다라";
        let width = 120.0_f32;

        let mut build = || {
            let mut builder = layout_cx.ranged_builder(&mut font_cx, text, 1.0, false);
            builder.push_default(StyleProperty::FontFamily(super::font_stack()));
            builder.push_default(StyleProperty::FontSize(16.0));
            builder.build(text)
        };

        // Baseline: the whole paragraph fits on one line at full width.
        let mut layout = build();
        layout.break_all_lines(Some(width));
        assert_eq!(layout.len(), 1, "baseline must fit on one line");
        let full_advance = layout.width();
        assert!(full_advance > 1.0, "the test font must shape glyphs");

        // Narrowing only the first line moves the trailing word down.
        let mut layout = build();
        let narrow = full_advance * 0.6;
        {
            let mut breaker = layout.break_lines();
            breaker.state_mut().set_layout_max_advance(width);
            let mut first = true;
            loop {
                breaker
                    .state_mut()
                    .set_line_max_advance(if first { narrow } else { width });
                if breaker.break_next().is_none() {
                    break;
                }
                first = false;
            }
            breaker.finish();
        }
        assert_eq!(
            layout.len(),
            2,
            "a narrowed first line must push trailing content down"
        );
        let first_line_advance = layout.lines().next().unwrap().metrics().advance;
        assert!(
            first_line_advance <= narrow + 0.001,
            "first line must respect the narrowed advance (got {first_line_advance}, max {narrow})"
        );
    }

    /// Pins the parley contract the tab-aware breaking path depends on: a
    /// caller-owned in-flow inline box contributes its width to line breaking,
    /// and that width can be changed on an already-shaped layout and re-broken
    /// deterministically without re-shaping.
    #[test]
    fn inline_box_width_participates_in_line_breaking() {
        use parley::style::StyleProperty;
        use parley::{InlineBox, InlineBoxKind};

        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();

        // The bundled test font is a Korean subset, so use covered glyphs.
        let text = "가나 다라";
        let width = 120.0_f32;

        // One shaping pass; the box sits between the two words.
        let mut builder = layout_cx.ranged_builder(&mut font_cx, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(super::font_stack()));
        builder.push_default(StyleProperty::FontSize(16.0));
        builder.push_inline_box(InlineBox {
            id: 7,
            kind: InlineBoxKind::InFlow,
            index: 6,
            width: 1.0,
            height: 0.0,
        });
        let mut layout = builder.build(text);

        layout.break_all_lines(Some(width));
        let narrow_lines = layout.len();
        assert!(
            layout.width() > 1.0,
            "the test font must actually shape glyphs (width={})",
            layout.width()
        );

        // Mutate the box width on the already-shaped layout and re-break.
        assert_eq!(layout.inline_boxes().len(), 1);
        layout.inline_boxes_mut()[0].width = width * 0.95;
        layout.break_all_lines(Some(width));
        let wide_lines = layout.len();

        assert_eq!(
            narrow_lines, 1,
            "a negligible box should leave the text on one line"
        );
        assert!(
            wide_lines > narrow_lines,
            "box width must push later content onto a new line \
             (narrow={narrow_lines}, wide={wide_lines})"
        );

        // Re-breaking is repeatable: the same width yields the same result, and
        // returning to the original width restores the original line count.
        layout.break_all_lines(Some(width));
        assert_eq!(
            layout.len(),
            wide_lines,
            "re-breaking must be deterministic"
        );
        layout.inline_boxes_mut()[0].width = 1.0;
        layout.break_all_lines(Some(width));
        assert_eq!(
            layout.len(),
            narrow_lines,
            "restoring the box width must restore the original breaking"
        );
    }

    #[test]
    fn image_layout_normalizes_rotation_and_uses_exact_quarter_turn_bounds() {
        let unrotated = image_layout(200, 100, None, 1_000.0, 1_000.0).unwrap();
        assert_eq!(unrotated.rotation_degrees, 0);
        assert_eq!(unrotated.image_w, 150.0);
        assert_eq!(unrotated.image_h, 75.0);
        assert_eq!(unrotated.bounds_w, 150.0);
        assert_eq!(unrotated.bounds_h, 75.0);

        for degrees in [90, 450, -270] {
            let rotated = image_layout(200, 100, Some(degrees), 1_000.0, 1_000.0).unwrap();
            assert_eq!(rotated.rotation_degrees, 90);
            assert_eq!(rotated.image_w, 150.0);
            assert_eq!(rotated.image_h, 75.0);
            assert_eq!(rotated.bounds_w, 75.0);
            assert_eq!(rotated.bounds_h, 150.0);
        }

        for degrees in [180, -180] {
            let rotated = image_layout(200, 100, Some(degrees), 1_000.0, 1_000.0).unwrap();
            assert_eq!(rotated.rotation_degrees, 180);
            assert_eq!(rotated.bounds_w, 150.0);
            assert_eq!(rotated.bounds_h, 75.0);
        }

        let rotated = image_layout(200, 100, Some(270), 1_000.0, 1_000.0).unwrap();
        assert_eq!(rotated.bounds_w, 75.0);
        assert_eq!(rotated.bounds_h, 150.0);
    }

    #[test]
    fn image_layout_fits_arbitrary_rotated_bounds_proportionally() {
        let rotated = image_layout(200, 100, Some(45), 1_000.0, 1_000.0).unwrap();
        let expected_bounds = 225.0 * std::f32::consts::FRAC_1_SQRT_2;
        assert_close(rotated.bounds_w, expected_bounds);
        assert_close(rotated.bounds_h, expected_bounds);

        let fitted = image_layout(200, 100, Some(90), 50.0, 1_000.0).unwrap();
        assert_close(fitted.image_w, 100.0);
        assert_close(fitted.image_h, 50.0);
        assert_close(fitted.bounds_w, 50.0);
        assert_close(fitted.bounds_h, 100.0);

        assert!(image_layout(0, 100, Some(90), 50.0, 100.0).is_none());
        assert!(image_layout(100, 100, Some(90), 0.0, 100.0).is_none());
        assert!(image_layout(100, 100, Some(90), f32::NAN, 100.0).is_none());
        assert!(
            image_layout(
                u32::MAX,
                u32::MAX,
                Some(45),
                f32::MIN_POSITIVE,
                f32::MIN_POSITIVE,
            )
            .is_none(),
            "underflowed fitted dimensions must be rejected"
        );
    }

    #[test]
    fn image_paint_transform_rotates_clockwise_within_visual_bounds() {
        let layout = image_layout(200, 100, Some(90), 1_000.0, 1_000.0).unwrap();
        let transform = image_paint_transform(layout, 10.0, 20.0);
        let map = |x: f32, y: f32| {
            (
                transform.sx() * x + transform.kx() * y + transform.tx(),
                transform.ky() * x + transform.sy() * y + transform.ty(),
            )
        };

        let top_left = map(0.0, 0.0);
        let top_right = map(layout.image_w, 0.0);
        let bottom_left = map(0.0, layout.image_h);
        assert_close(top_left.0, 85.0);
        assert_close(top_left.1, 20.0);
        assert_close(top_right.0, 85.0);
        assert_close(top_right.1, 170.0);
        assert_close(bottom_left.0, 10.0);
        assert_close(bottom_left.1, 20.0);
    }

    #[test]
    fn rotated_image_bounds_drive_block_pagination() {
        let bytes = vec![0; 100 * 800 * 4];
        let image = Image {
            bytes: Some(bytes),
            mime: Some(crate::image::MIME_RAW_RGBA.to_string()),
            width_px: Some(100),
            height_px: Some(800),
            ..Image::default()
        };
        let model = |rotation_degrees| DocModel {
            blocks: vec![
                Block::Image(Image {
                    rotation_degrees,
                    ..image.clone()
                }),
                Block::Image(Image {
                    rotation_degrees,
                    ..image.clone()
                }),
            ],
            ..DocModel::default()
        };

        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let unrotated = super::layout_pages_with_fonts(&model(None), &fonts).unwrap();
        let rotated = super::layout_pages_with_fonts(&model(Some(90)), &fonts).unwrap();
        assert_eq!(unrotated.pages, 2);
        assert_eq!(unrotated.block_pages, vec![Some(1), Some(2)]);
        assert_eq!(rotated.pages, 1);
        assert_eq!(rotated.block_pages, vec![Some(1), Some(1)]);
    }

    #[test]
    fn rotated_images_fit_and_advance_within_active_columns() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let page = PageSetup {
            width_pt: 220.0,
            height_pt: 200.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        };
        let image = Image {
            bytes: Some(vec![0; 200 * 200 * 4]),
            mime: Some(crate::image::MIME_RAW_RGBA.to_string()),
            width_px: Some(200),
            height_px: Some(200),
            rotation_degrees: Some(90),
            ..Image::default()
        };
        let model = DocModel {
            blocks: vec![Block::Image(image.clone()), Block::Image(image)],
            setup: crate::model::DocSetup {
                page,
                columns: Some(2),
                ..Default::default()
            },
            ..DocModel::default()
        };
        let geom = Geom::from_setup(&page);
        let mut capture = LayoutCapture::default();
        let flow = super::collect_pdf_flow_items(
            &model,
            geom,
            &mut tcx,
            &mut capture,
            super::SourceRenderHints::default(),
            &[],
            None,
        );
        let layouts = flow
            .iter()
            .filter_map(|item| match item {
                FlowItem::Picture { layout, .. } => Some(*layout),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(layouts.len(), 2);
        for layout in layouts {
            assert_close(layout.bounds_w, 81.0);
            assert_close(layout.bounds_h, 81.0);
        }

        let setup = SectionSetup::from(&model.setup);
        let pagination = paginate(flow, geom, &setup);
        assert_eq!(pagination.pages.len(), 1);
        let pictures = pagination.pages[0]
            .iter()
            .filter_map(|placed| {
                matches!(&placed.item, FlowItem::Picture { .. }).then_some((placed.x, placed.width))
            })
            .collect::<Vec<_>>();
        assert_eq!(pictures.len(), 2);
        assert_close(pictures[0].0, 0.0);
        assert_close(pictures[0].1, 81.0);
        assert!(pictures[1].0 > 90.0);
        assert_close(pictures[1].1, 81.0);
    }

    fn paragraph_lines_with_marker(
        props: ParaProps,
        runs: Vec<Run>,
        marker: Option<&str>,
    ) -> Vec<LineLayout> {
        paragraph_lines_with_marker_and_tabs(props, runs, marker, &[])
    }

    fn paragraph_lines_with_marker_and_tabs(
        props: ParaProps,
        runs: Vec<Run>,
        marker: Option<&str>,
        tab_stops: &[TabStop],
    ) -> Vec<LineLayout> {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();
        layout_paragraph(
            &Paragraph { props, runs },
            &mut flow,
            marker,
            tab_stops,
            None,
            geom,
            &mut tcx,
            &mut capture,
        );
        flow.into_iter()
            .filter_map(|item| match item {
                FlowItem::Line(line) => Some(line),
                _ => None,
            })
            .collect()
    }

    fn paragraph_lines(props: ParaProps, runs: Vec<Run>) -> Vec<LineLayout> {
        paragraph_lines_with_marker(props, runs, None)
    }

    fn text_bounds(line: &LineLayout, byte_range: std::ops::Range<usize>) -> Option<(f32, f32)> {
        let mut left = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        for run in &line.runs {
            let mut cursor = run.x;
            for glyph in &run.glyphs {
                let advance = glyph.x_advance * run.size;
                if glyph.text_range.start < byte_range.end
                    && byte_range.start < glyph.text_range.end
                {
                    let glyph_x = cursor + glyph.x_offset * run.size;
                    left = left.min(glyph_x);
                    right = right.max(glyph_x + advance);
                }
                cursor += advance;
            }
        }
        left.is_finite().then_some((left, right))
    }

    fn tab_aligned_position(
        line: &LineLayout,
        byte_range: std::ops::Range<usize>,
        alignment: TabAlignment,
    ) -> f32 {
        let bounds = text_bounds(line, byte_range).expect("measured tab field glyphs");
        line.x_indent
            + match alignment {
                TabAlignment::Left | TabAlignment::Decimal => bounds.0,
                TabAlignment::Center => (bounds.0 + bounds.1) / 2.0,
                TabAlignment::Right => bounds.1,
                TabAlignment::Clear => unreachable!(),
            }
    }

    type ParagraphLineMetric = (f32, f32, Option<(usize, usize)>);

    fn paragraph_line_metrics(props: ParaProps, runs: Vec<Run>) -> Vec<ParagraphLineMetric> {
        paragraph_lines(props, runs)
            .into_iter()
            .map(|line| {
                (
                    line.height,
                    line.x_indent + line.runs.first().map(|run| run.x).unwrap_or(0.0),
                    line.char_range.map(|range| (range.start, range.end)),
                )
            })
            .collect()
    }

    fn shaped_run_sizes(text: &str, props: CharProps) -> Vec<f32> {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        shape(
            text,
            StyledText::plain(&[(0, text.len(), props)]),
            None,
            parley::layout::Alignment::Start,
            320.0,
            &mut tcx,
        )
        .into_iter()
        .flat_map(|line| line.runs.into_iter().map(|run| run.size))
        .collect()
    }

    #[test]
    fn small_caps_and_vertical_alignment_use_reduced_glyph_sizes() {
        let baseline = shaped_run_sizes("ABC", CharProps::default());
        let small_caps = shaped_run_sizes(
            "ABC",
            CharProps {
                small_caps: true,
                ..CharProps::default()
            },
        );
        let superscript = shaped_run_sizes(
            "ABC",
            CharProps {
                vert_align: VertAlign::Super,
                ..CharProps::default()
            },
        );
        let subscript = shaped_run_sizes(
            "ABC",
            CharProps {
                vert_align: VertAlign::Sub,
                ..CharProps::default()
            },
        );

        assert_eq!(baseline.len(), 1);
        assert_eq!(small_caps.len(), 1);
        assert_eq!(superscript.len(), 1);
        assert_eq!(subscript.len(), 1);
        assert!(small_caps[0] < baseline[0] * 0.85);
        assert!(superscript[0] < baseline[0] * 0.75);
        assert!(subscript[0] < baseline[0] * 0.75);
    }

    #[test]
    fn small_caps_keep_authored_uppercase_at_full_size() {
        let lines = paragraph_lines(
            ParaProps::default(),
            vec![Run {
                text: "aA".to_string(),
                props: CharProps {
                    small_caps: true,
                    ..CharProps::default()
                },
                ..Run::default()
            }],
        );
        let sizes = lines[0].runs.iter().map(|run| run.size).collect::<Vec<_>>();

        assert_eq!(sizes.len(), 2);
        assert!(sizes[0] < sizes[1] * 0.85);
    }

    #[test]
    fn bidi_paragraph_forces_rtl_base_for_latin_and_numbers() {
        let lines = paragraph_lines(
            ParaProps {
                align: Align::Right,
                bidi: true,
                ..ParaProps::default()
            },
            vec![Run {
                text: "123 ABC".to_string(),
                ..Run::default()
            }],
        );
        let first = &lines[0].runs[0];

        assert!(first.text.starts_with('\u{200f}'));
        assert!(
            first.x > 100.0,
            "resolved RTL paragraph start should use the right edge"
        );
    }

    #[test]
    fn rtl_run_is_isolated_inside_ltr_paragraph() {
        let lines = paragraph_lines(
            ParaProps::default(),
            vec![
                Run {
                    text: "left ".to_string(),
                    ..Run::default()
                },
                Run {
                    text: "ABC 123".to_string(),
                    props: CharProps {
                        rtl: true,
                        ..CharProps::default()
                    },
                    ..Run::default()
                },
                Run {
                    text: " tail".to_string(),
                    ..Run::default()
                },
            ],
        );
        let shaped_text = &lines[0].runs[0].text;

        assert!(shaped_text.contains("\u{2067}ABC 123\u{2069}"));
        assert!(lines.iter().flat_map(|line| &line.runs).all(|run| {
            run.glyphs.iter().all(|glyph| {
                !run.text[glyph.text_range.clone()]
                    .chars()
                    .any(|ch| matches!(ch, '\u{200f}' | '\u{2067}' | '\u{2069}'))
            })
        }));
    }

    #[test]
    fn rtl_controls_do_not_shift_source_character_ranges() {
        let lines = paragraph_lines(
            ParaProps {
                align: Align::Right,
                bidi: true,
                ..ParaProps::default()
            },
            vec![Run {
                text: "ABC".to_string(),
                props: CharProps {
                    rtl: true,
                    ..CharProps::default()
                },
                ..Run::default()
            }],
        );

        assert_eq!(
            lines[0].char_range.map(|range| (range.start, range.end)),
            Some((0, 3))
        );
    }

    #[test]
    fn list_marker_does_not_shift_source_character_ranges() {
        let lines = paragraph_lines_with_marker(
            ParaProps::default(),
            vec![Run {
                text: "ABC".to_string(),
                ..Run::default()
            }],
            Some("1."),
        );

        assert_eq!(
            lines[0].char_range.map(|range| (range.start, range.end)),
            Some((0, 3))
        );
    }

    #[test]
    fn bidi_list_marker_uses_rtl_paragraph_start_edge() {
        let lines = paragraph_lines_with_marker(
            ParaProps {
                align: Align::Right,
                bidi: true,
                ..ParaProps::default()
            },
            vec![Run {
                text: "ABC".to_string(),
                ..Run::default()
            }],
            Some("1."),
        );
        let first = &lines[0].runs[0];

        assert!(first.text.starts_with("\u{200f}1. "));
        assert!(first.x > 100.0);
    }

    #[test]
    fn bidi_controls_do_not_make_hidden_text_visible() {
        let lines = paragraph_lines(
            ParaProps {
                bidi: true,
                ..ParaProps::default()
            },
            vec![Run {
                text: "hidden".to_string(),
                props: CharProps {
                    hidden: true,
                    rtl: true,
                    ..CharProps::default()
                },
                ..Run::default()
            }],
        );

        assert!(lines.is_empty());
    }

    #[test]
    fn vertical_alignment_shifts_the_glyph_baseline() {
        let shaped_shift = |vert_align| {
            let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
            let mut font_cx = strict_font_context(&fonts);
            let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
            let mut font_cache = HashMap::new();
            let mut tcx = TextCx {
                font_cx: &mut font_cx,
                layout_cx: &mut layout_cx,
                font_cache: &mut font_cache,
            };
            shape(
                "x",
                StyledText::plain(&[(
                    0,
                    1,
                    CharProps {
                        vert_align,
                        ..CharProps::default()
                    },
                )]),
                None,
                parley::layout::Alignment::Start,
                100.0,
                &mut tcx,
            )[0]
            .runs[0]
                .baseline_shift
        };

        assert!(shaped_shift(VertAlign::Super) < 0.0);
        assert!(shaped_shift(VertAlign::Sub) > 0.0);
        assert_eq!(shaped_shift(VertAlign::Baseline), 0.0);
    }

    #[test]
    fn highlight_and_text_decorations_reach_draw_runs() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let line = shape(
            "paint",
            StyledText::plain(&[(
                0,
                5,
                CharProps {
                    underline: true,
                    strike: true,
                    highlight: Some("darkYellow".to_string()),
                    ..CharProps::default()
                },
            )]),
            None,
            parley::layout::Alignment::Start,
            100.0,
            &mut tcx,
        )
        .remove(0);
        let run = &line.runs[0];

        assert_eq!(run.highlight, Some(rgb::Color::new(0x80, 0x80, 0x00)));
        assert!(run.underline.is_some());
        assert!(run.strikethrough.is_some());
    }

    #[test]
    fn paragraph_shading_reaches_each_laid_out_line() {
        let lines = paragraph_lines(
            ParaProps {
                shading: Some(Color::rgb(0xEE, 0xF1, 0xF4)),
                ..ParaProps::default()
            },
            vec![Run {
                text: "A paragraph background".to_string(),
                ..Run::default()
            }],
        );

        assert!(!lines.is_empty());
        assert!(lines.iter().all(|line| {
            line.background.is_some_and(|background| {
                background.color == rgb::Color::new(0xEE, 0xF1, 0xF4) && background.width > 0.0
            })
        }));
    }

    #[test]
    fn horizontal_tab_advances_to_default_word_stop() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let text = "A\tB";
        let line = shape(
            text,
            StyledText::plain(&[(0, text.len(), CharProps::default())]),
            None,
            parley::layout::Alignment::Left,
            320.0,
            &mut tcx,
        )
        .remove(0);
        let mut glyph_debug = Vec::new();
        let mut b_x = None;
        for run in &line.runs {
            let mut x = run.x;
            for glyph in &run.glyphs {
                glyph_debug.push((glyph.text_range.clone(), x, glyph.x_advance * run.size));
                if glyph.text_range.contains(&2) {
                    b_x = Some(x + glyph.x_offset * run.size);
                }
                x += glyph.x_advance * run.size;
            }
        }
        let b_x = b_x.expect("B glyph");

        assert!(
            (b_x - 36.0).abs() <= 1.0,
            "b_x={b_x}, glyphs={glyph_debug:?}"
        );
    }

    #[test]
    fn rtl_start_tab_advances_from_the_right_page_margin() {
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps {
                align: Align::Right,
                bidi: true,
                ..ParaProps::default()
            },
            vec![Run {
                text: "א\tב".to_string(),
                ..Run::default()
            }],
            None,
            &[],
        );
        let line = &lines[0];
        let rendered = &line.runs[0].text;
        let field_start = rendered.find('ב').expect("tab field text");
        let bounds =
            text_bounds(line, field_start..field_start + 'ב'.len_utf8()).expect("field glyph");
        let actual = line.x_indent + bounds.1;

        assert!(
            (actual - 144.0).abs() <= 1.5,
            "RTL start field must end 36pt from the 180pt right margin: actual={actual}, bounds={bounds:?}"
        );
    }

    #[test]
    fn rtl_explicit_start_tabs_keep_page_margin_coordinates_under_indents() {
        for indent in [
            Indent {
                right_pt: Some(20.0),
                ..Indent::default()
            },
            Indent {
                left_pt: Some(20.0),
                ..Indent::default()
            },
        ] {
            let lines = paragraph_lines_with_marker_and_tabs(
                ParaProps {
                    align: Align::Right,
                    bidi: true,
                    indent,
                    ..ParaProps::default()
                },
                vec![Run {
                    text: "א\tב".to_string(),
                    ..Run::default()
                }],
                None,
                &[TabStop {
                    position_pt: 100.0,
                    alignment: TabAlignment::Left,
                }],
            );
            let line = &lines[0];
            let rendered = &line.runs[0].text;
            let field_start = rendered.find('ב').expect("tab field text");
            let actual = tab_aligned_position(
                line,
                field_start..field_start + 'ב'.len_utf8(),
                TabAlignment::Right,
            );

            assert!(
                (actual - 80.0).abs() <= 1.5,
                "a stop 100pt from the 180pt right margin must land at x=80: indent={indent:?}, actual={actual}"
            );
        }
    }

    #[test]
    fn rtl_default_tabs_advance_multiple_fields_from_right_to_left() {
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps {
                align: Align::Right,
                bidi: true,
                ..ParaProps::default()
            },
            vec![Run {
                text: "א\tב\tג".to_string(),
                ..Run::default()
            }],
            None,
            &[],
        );
        let line = &lines[0];
        let rendered = &line.runs[0].text;
        let beta = rendered.find('ב').expect("first tab field");
        let gamma = rendered.find('ג').expect("second tab field");
        let beta_right =
            tab_aligned_position(line, beta..beta + 'ב'.len_utf8(), TabAlignment::Right);
        let gamma_right =
            tab_aligned_position(line, gamma..gamma + 'ג'.len_utf8(), TabAlignment::Right);

        assert!((beta_right - 144.0).abs() <= 1.5, "beta={beta_right}");
        assert!((gamma_right - 108.0).abs() <= 1.5, "gamma={gamma_right}");
    }

    #[test]
    fn rtl_start_tabs_preserve_segment_paint_and_source_ranges() {
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps {
                align: Align::Right,
                bidi: true,
                ..ParaProps::default()
            },
            vec![
                Run {
                    text: "א\t".to_string(),
                    ..Run::default()
                },
                Run {
                    text: "ב".to_string(),
                    props: CharProps {
                        highlight: Some("yellow".to_string()),
                        ..CharProps::default()
                    },
                    ..Run::default()
                },
            ],
            None,
            &[TabStop {
                position_pt: 100.0,
                alignment: TabAlignment::Left,
            }],
        );
        let line = &lines[0];
        let field_start = line.runs[0].text.find('ב').expect("tab field text");
        let field_right = tab_aligned_position(
            line,
            field_start..field_start + 'ב'.len_utf8(),
            TabAlignment::Right,
        );

        assert!((field_right - 80.0).abs() <= 1.5);
        assert_eq!(
            line.char_range.map(|range| (range.start, range.end)),
            Some((0, 3))
        );
        assert!(line
            .runs
            .iter()
            .any(|run| run.highlight == Some(rgb::Color::new(0xFF, 0xFF, 0x00))));
    }

    #[test]
    fn rtl_explicit_center_end_and_decimal_tabs_use_their_stops() {
        let cases = [
            ("א\tב", TabAlignment::Center, TabAlignment::Center),
            ("א\tאב", TabAlignment::Right, TabAlignment::Left),
            ("א\t12.34", TabAlignment::Decimal, TabAlignment::Right),
        ];
        for (text, alignment, measured_alignment) in cases {
            let lines = paragraph_lines_with_marker_and_tabs(
                ParaProps {
                    align: Align::Right,
                    bidi: true,
                    ..ParaProps::default()
                },
                vec![Run {
                    text: text.to_string(),
                    ..Run::default()
                }],
                None,
                &[TabStop {
                    position_pt: 100.0,
                    alignment,
                }],
            );
            let rendered = &lines[0].runs[0].text;
            let field_start = if alignment == TabAlignment::Decimal {
                rendered.find('.').expect("decimal field")
            } else {
                rendered.find('\t').expect("tab marker") + '\t'.len_utf8()
            };
            let field_end = if alignment == TabAlignment::Decimal {
                field_start + '.'.len_utf8()
            } else {
                rendered.len()
            };
            let bounds = text_bounds(&lines[0], field_start..field_end).expect("field glyph");
            let actual = lines[0].x_indent
                + match measured_alignment {
                    TabAlignment::Left | TabAlignment::Decimal => bounds.0,
                    TabAlignment::Center => (bounds.0 + bounds.1) / 2.0,
                    TabAlignment::Right => bounds.1,
                    TabAlignment::Clear => unreachable!(),
                };
            assert!(
                (actual - 80.0).abs() <= 1.5,
                "alignment={alignment:?} actual={actual}"
            );
        }
    }

    #[test]
    fn rtl_unreachable_non_start_custom_tabs_keep_the_baseline() {
        let field_bounds = |position_pt| {
            let lines = paragraph_lines_with_marker_and_tabs(
                ParaProps {
                    align: Align::Right,
                    bidi: true,
                    ..ParaProps::default()
                },
                vec![Run {
                    text: "א\tב".to_string(),
                    ..Run::default()
                }],
                None,
                &[TabStop {
                    position_pt,
                    alignment: TabAlignment::Center,
                }],
            );
            let rendered = &lines[0].runs[0].text;
            let field_start = rendered.find('ב').expect("tab field text");
            text_bounds(&lines[0], field_start..field_start + 'ב'.len_utf8()).expect("field glyph")
        };

        assert_eq!(field_bounds(1.0), field_bounds(2.0));
    }

    #[test]
    fn natural_rtl_text_without_paragraph_bidi_keeps_existing_tab_behavior() {
        let field_bounds = |position_pt| {
            let lines = paragraph_lines_with_marker_and_tabs(
                ParaProps {
                    align: Align::Right,
                    ..ParaProps::default()
                },
                vec![Run {
                    text: "א\tב".to_string(),
                    ..Run::default()
                }],
                None,
                &[TabStop {
                    position_pt,
                    alignment: TabAlignment::Left,
                }],
            );
            let rendered = &lines[0].runs[0].text;
            let field_start = rendered.find('ב').expect("tab field text");
            text_bounds(&lines[0], field_start..field_start + 'ב'.len_utf8()).expect("field glyph")
        };

        assert_eq!(field_bounds(60.0), field_bounds(120.0));
    }

    #[test]
    fn rtl_tab_advances_reflow_unfitting_content_deterministically() {
        let paragraph = || {
            paragraph_lines_with_marker_and_tabs(
                ParaProps {
                    align: Align::Right,
                    bidi: true,
                    ..ParaProps::default()
                },
                vec![Run {
                    text: "\t\t\t\tאבגדה".to_string(),
                    ..Run::default()
                }],
                None,
                &[],
            )
        };
        let first = paragraph();
        let second = paragraph();

        assert_eq!(first.len(), 2);
        assert_eq!(first.len(), second.len());
        for (left, right) in first.iter().zip(&second) {
            assert_eq!(
                left.char_range.map(|range| (range.start, range.end)),
                right.char_range.map(|range| (range.start, range.end))
            );
            assert!(left.runs.iter().all(|run| run.x >= -0.5));
        }
    }

    /// The end of the last glyph on a line, in paragraph-box coordinates.
    fn line_end_x(line: &LineLayout) -> f32 {
        let mut end: f32 = 0.0;
        for run in &line.runs {
            let mut x = run.x;
            for glyph in &run.glyphs {
                x += glyph.x_advance * run.size;
            }
            end = end.max(x);
        }
        end
    }

    #[test]
    fn tab_advances_move_unfitting_content_to_the_next_line() {
        // The content box is 180pt wide at page margin 20pt, so the default
        // 36pt grid stops land at 36/72/108/144pt absolute. Four tabs leave the
        // cursor 124pt into the box, which no longer leaves room for the word.
        let text = "\t\t\t\t가나다라";
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps::default(),
            vec![Run {
                text: text.to_string(),
                ..Run::default()
            }],
            None,
            &[],
        );
        let ends: Vec<f32> = lines.iter().map(line_end_x).collect();
        assert_eq!(
            lines.len(),
            2,
            "tab-driven overflow must wrap instead of running past the box (ends={ends:?})"
        );
        for (index, end) in ends.iter().enumerate() {
            assert!(
                *end <= 180.0 + 0.5,
                "line {index} must stay inside the 180pt box (ends={ends:?})"
            );
        }
    }

    #[test]
    fn fitting_tab_advances_do_not_reflow_the_line() {
        // The same four-stop grid, but the trailing word still fits, so the
        // reservation pass must leave the original single line alone.
        let text = "\t가나";
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps::default(),
            vec![Run {
                text: text.to_string(),
                ..Run::default()
            }],
            None,
            &[],
        );
        let ends: Vec<f32> = lines.iter().map(line_end_x).collect();
        assert_eq!(
            lines.len(),
            1,
            "a tab whose field fits must not move content down (ends={ends:?})"
        );
    }

    #[test]
    fn tab_reflow_is_deterministic_and_bounded_in_a_narrow_box() {
        // Indents shrink the paragraph box far below the tab grid, so the
        // reservation cannot be satisfied. This must stay panic-free, keep
        // every glyph, and repeat identically.
        let props = || ParaProps {
            indent: Indent {
                left_pt: Some(70.0),
                right_pt: Some(70.0),
                ..Indent::default()
            },
            ..ParaProps::default()
        };
        let text = "\t\t가나다라";
        let run = || {
            vec![Run {
                text: text.to_string(),
                ..Run::default()
            }]
        };
        let first = paragraph_lines_with_marker_and_tabs(props(), run(), None, &[]);
        let second = paragraph_lines_with_marker_and_tabs(props(), run(), None, &[]);

        assert!(!first.is_empty(), "a narrow box must still produce lines");
        assert_eq!(
            first.len(),
            second.len(),
            "tab reflow must be deterministic across runs"
        );
        let geometry = |lines: &[LineLayout]| {
            lines
                .iter()
                .map(|line| {
                    line.runs
                        .iter()
                        .map(|run| {
                            (
                                run.x.to_bits(),
                                run.glyphs
                                    .iter()
                                    .map(|g| g.x_advance.to_bits())
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            geometry(&first),
            geometry(&second),
            "repeated shaping must be byte-identical"
        );
    }

    #[test]
    fn tab_driven_wrapping_reaches_emitted_page_counts() {
        // A lone two-line paragraph is never split, so page effects only show
        // with enough paragraphs to fill pages. Twelve one-line paragraphs fill
        // four pages; the same count wrapped to two lines by a tab reservation
        // must fill eight.
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let pages_of = |text: &str| {
            let model = DocModel {
                setup: crate::model::DocSetup {
                    page: PageSetup {
                        width_pt: 220.0,
                        height_pt: 100.0,
                        margin_pt: 20.0,
                        ..PageSetup::default()
                    },
                    ..crate::model::DocSetup::default()
                },
                blocks: (0..12)
                    .map(|_| {
                        Block::Paragraph(Paragraph {
                            props: ParaProps::default(),
                            runs: vec![Run {
                                text: text.to_string(),
                                ..Run::default()
                            }],
                        })
                    })
                    .collect(),
                ..DocModel::default()
            };
            super::layout_pages_with_fonts(&model, &fonts)
                .unwrap()
                .pages
        };

        assert_eq!(
            pages_of("\t가나"),
            4,
            "a fitting tab field stays on one line"
        );
        assert_eq!(
            pages_of("\t\t\t\t가나다라"),
            8,
            "content moved down by a tab reservation must reach the page count"
        );
    }

    #[test]
    fn default_tab_reflow_only_applies_to_left_and_start_aligned_ltr_text() {
        // Default-tab reflow stays scoped to left/start alignment. Explicit
        // custom stops use the same bounded reservation path for other LTR
        // paragraph alignments.
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let text = "\t\t\t\t가나다라";
        let centered = shape(
            text,
            StyledText::plain(&[(0, text.len(), CharProps::default())]),
            None,
            parley::layout::Alignment::Center,
            180.0,
            &mut tcx,
        );
        assert_eq!(
            centered.len(),
            1,
            "centered text must keep parley's breaking untouched"
        );
    }

    #[test]
    fn explicit_tabs_apply_left_center_right_and_decimal_alignment() {
        let cases = [
            ("A\tLEFT", 2..6, TabAlignment::Left, 90.0),
            ("A\tCENTER", 2..8, TabAlignment::Center, 100.0),
            ("A\tRIGHT", 2..7, TabAlignment::Right, 150.0),
            ("A\t12.34", 4..5, TabAlignment::Decimal, 110.0),
            ("A\t1,234.56", 7..8, TabAlignment::Decimal, 110.0),
        ];

        for left_pt in [None, Some(20.0)] {
            for (text, measured, alignment, position_pt) in cases.clone() {
                let lines = paragraph_lines_with_marker_and_tabs(
                    ParaProps {
                        indent: Indent {
                            left_pt,
                            ..Indent::default()
                        },
                        ..ParaProps::default()
                    },
                    vec![Run {
                        text: text.to_string(),
                        ..Run::default()
                    }],
                    None,
                    &[TabStop {
                        position_pt,
                        alignment,
                    }],
                );
                let actual = tab_aligned_position(&lines[0], measured, alignment);
                assert!(
                    (actual - position_pt).abs() <= 1.5,
                    "left={left_pt:?} alignment={alignment:?} actual={actual} expected={position_pt}"
                );
            }
        }
    }

    #[test]
    fn explicit_left_tabs_in_non_left_paragraph_alignments_use_their_stops() {
        for (align, position_pt) in [
            (Align::Center, 100.0),
            (Align::Right, 170.0),
            (Align::Justify, 100.0),
        ] {
            let lines = paragraph_lines_with_marker_and_tabs(
                ParaProps {
                    align,
                    ..ParaProps::default()
                },
                vec![Run {
                    text: "A\tB".to_string(),
                    ..Run::default()
                }],
                None,
                &[TabStop {
                    position_pt,
                    alignment: TabAlignment::Left,
                }],
            );
            let field_start = "A\t".len();
            let actual =
                tab_aligned_position(&lines[0], field_start..field_start + 1, TabAlignment::Left);
            assert!(
                (actual - position_pt).abs() <= 1.5,
                "align={align:?} actual={actual} expected={position_pt}"
            );

            let baseline = paragraph_lines_with_marker_and_tabs(
                ParaProps {
                    align,
                    ..ParaProps::default()
                },
                vec![Run {
                    text: "A\tB".to_string(),
                    ..Run::default()
                }],
                None,
                &[],
            );
            let unreachable = paragraph_lines_with_marker_and_tabs(
                ParaProps {
                    align,
                    ..ParaProps::default()
                },
                vec![Run {
                    text: "A\tB".to_string(),
                    ..Run::default()
                }],
                None,
                &[TabStop {
                    position_pt: 1.0,
                    alignment: TabAlignment::Left,
                }],
            );
            let baseline_position = tab_aligned_position(
                &baseline[0],
                field_start..field_start + 1,
                TabAlignment::Left,
            );
            let unreachable_position = tab_aligned_position(
                &unreachable[0],
                field_start..field_start + 1,
                TabAlignment::Left,
            );
            assert!(
                (baseline_position - unreachable_position).abs() <= 1.5,
                "unreachable custom stop changed default behavior for {align:?}"
            );
        }
    }

    #[test]
    fn explicit_tabs_use_the_indented_paragraph_box_and_preserve_paint_ranges() {
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps {
                indent: Indent {
                    left_pt: Some(20.0),
                    right_pt: Some(20.0),
                    first_line_pt: Some(10.0),
                    ..Indent::default()
                },
                ..ParaProps::default()
            },
            vec![
                Run {
                    text: "A\t".to_string(),
                    ..Run::default()
                },
                Run {
                    text: "B".to_string(),
                    props: CharProps {
                        highlight: Some("yellow".to_string()),
                        ..CharProps::default()
                    },
                    ..Run::default()
                },
            ],
            None,
            &[TabStop {
                position_pt: 100.0,
                alignment: TabAlignment::Left,
            }],
        );
        let b_bounds = text_bounds(&lines[0], 2..3).expect("B glyph");

        assert!((lines[0].x_indent - 20.0).abs() < 0.1);
        assert!(
            (lines[0].x_indent + b_bounds.0 - 100.0).abs() <= 1.5,
            "x_indent={} B bounds={b_bounds:?}",
            lines[0].x_indent
        );
        assert_eq!(
            lines[0].char_range.map(|range| (range.start, range.end)),
            Some((0, 3))
        );
        assert!(lines[0]
            .runs
            .iter()
            .any(|run| run.highlight == Some(rgb::Color::new(0xFF, 0xFF, 0x00))));
    }

    #[test]
    fn explicit_tabs_keep_page_margin_coordinates_under_first_and_hanging_indents() {
        let indents = [
            Indent {
                left_pt: Some(20.0),
                first_line_pt: Some(10.0),
                ..Indent::default()
            },
            Indent {
                left_pt: Some(40.0),
                hanging_pt: Some(20.0),
                ..Indent::default()
            },
        ];
        let fields = [
            ("A\tLEFT", 2..6, TabAlignment::Left),
            ("A\tCENTER", 2..8, TabAlignment::Center),
            ("A\tRIGHT", 2..7, TabAlignment::Right),
            ("A\t12.34", 4..5, TabAlignment::Decimal),
        ];

        for indent in indents {
            for (text, measured, alignment) in fields.clone() {
                let lines = paragraph_lines_with_marker_and_tabs(
                    ParaProps {
                        indent,
                        ..ParaProps::default()
                    },
                    vec![Run {
                        text: text.to_string(),
                        ..Run::default()
                    }],
                    None,
                    &[TabStop {
                        position_pt: 100.0,
                        alignment,
                    }],
                );
                let actual = tab_aligned_position(&lines[0], measured, alignment);

                assert!(
                    (actual - 100.0).abs() <= 1.5,
                    "indent={indent:?} alignment={alignment:?} actual={actual}"
                );
            }
        }
    }

    #[test]
    fn hanging_indent_continuation_lines_keep_page_margin_tab_coordinates() {
        let text = "first line\nA\tB";
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps {
                indent: Indent {
                    left_pt: Some(40.0),
                    hanging_pt: Some(20.0),
                    ..Indent::default()
                },
                ..ParaProps::default()
            },
            vec![Run {
                text: text.to_string(),
                ..Run::default()
            }],
            None,
            &[TabStop {
                position_pt: 100.0,
                alignment: TabAlignment::Left,
            }],
        );
        let b_start = text.find('B').unwrap();
        let continuation = lines.last().expect("continuation line");
        let actual = tab_aligned_position(continuation, b_start..b_start + 1, TabAlignment::Left);

        assert!(
            (actual - 100.0).abs() <= 1.5,
            "actual={actual} lines={}",
            lines.len()
        );
    }

    #[test]
    fn explicit_tabs_fall_back_when_aligned_fields_cross_the_paragraph_box() {
        let cases = [
            ("A\tFIELD", TabAlignment::Left, 160.0),
            ("A\tCENTERED", TabAlignment::Center, 150.0),
            ("A\t12.3456789", TabAlignment::Decimal, 150.0),
            ("A\tFIELD", TabAlignment::Right, 170.0),
        ];

        for (text, alignment, position_pt) in cases {
            let lines = paragraph_lines_with_marker_and_tabs(
                ParaProps {
                    indent: Indent {
                        left_pt: Some(20.0),
                        right_pt: Some(20.0),
                        ..Indent::default()
                    },
                    ..ParaProps::default()
                },
                vec![Run {
                    text: text.to_string(),
                    ..Run::default()
                }],
                None,
                &[TabStop {
                    position_pt,
                    alignment,
                }],
            );
            let field_start = text.find('\t').unwrap() + 1;
            let bounds = text_bounds(&lines[0], field_start..text.len()).expect("field glyphs");
            let actual = lines[0].x_indent + bounds.0;

            assert!(
                (actual - DEFAULT_TAB_STOP_PT).abs() <= 1.5,
                "alignment={alignment:?} stop={position_pt} actual={actual}"
            );
        }
    }

    #[test]
    fn default_tabs_use_the_page_margin_grid_under_paragraph_indents() {
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps {
                indent: Indent {
                    left_pt: Some(20.0),
                    ..Indent::default()
                },
                ..ParaProps::default()
            },
            vec![Run {
                text: "A\tB".to_string(),
                ..Run::default()
            }],
            None,
            &[],
        );
        let b_bounds = text_bounds(&lines[0], 2..3).expect("B glyph");
        let actual = lines[0].x_indent + b_bounds.0;

        assert!(
            (actual - DEFAULT_TAB_STOP_PT).abs() <= 1.5,
            "actual={actual}"
        );
    }

    #[test]
    fn default_tab_targets_are_margin_anchored_and_clamped_to_the_paragraph_box() {
        assert_eq!(super::default_tab_field_start(110.0, 140.0, 20.0), 124.0);
        assert_eq!(super::default_tab_field_start(135.0, 140.0, 20.0), 140.0);
    }

    #[test]
    fn explicit_tab_before_the_indented_cursor_falls_back_to_the_page_margin_grid() {
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps {
                indent: Indent {
                    left_pt: Some(50.0),
                    ..Indent::default()
                },
                ..ParaProps::default()
            },
            vec![Run {
                text: "A\tB".to_string(),
                ..Run::default()
            }],
            None,
            &[TabStop {
                position_pt: DEFAULT_TAB_STOP_PT,
                alignment: TabAlignment::Left,
            }],
        );
        let b_bounds = text_bounds(&lines[0], 2..3).expect("B glyph");
        let actual = lines[0].x_indent + b_bounds.0;

        assert!(
            (actual - DEFAULT_TAB_STOP_PT * 2.0).abs() <= 1.5,
            "actual={actual}"
        );
    }

    #[test]
    fn explicit_tab_past_the_paragraph_box_falls_back_without_overflow() {
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps {
                indent: Indent {
                    left_pt: Some(20.0),
                    right_pt: Some(20.0),
                    ..Indent::default()
                },
                ..ParaProps::default()
            },
            vec![Run {
                text: "A\tB".to_string(),
                ..Run::default()
            }],
            None,
            &[TabStop {
                position_pt: 1_000.0,
                alignment: TabAlignment::Left,
            }],
        );
        let b_bounds = text_bounds(&lines[0], 2..3).expect("B glyph");
        let absolute_right = lines[0].x_indent + b_bounds.1;
        let absolute_left = lines[0].x_indent + b_bounds.0;

        assert!(
            (absolute_left - DEFAULT_TAB_STOP_PT).abs() <= 1.5,
            "absolute_left={absolute_left}"
        );
        assert!(
            absolute_right <= 160.0,
            "x_indent={} B bounds={b_bounds:?}",
            lines[0].x_indent
        );
    }

    #[test]
    fn right_tab_accepts_a_field_ending_at_the_paragraph_box_edge() {
        let text = "A\tEDGE";
        let lines = paragraph_lines_with_marker_and_tabs(
            ParaProps {
                indent: Indent {
                    left_pt: Some(20.0),
                    right_pt: Some(20.0),
                    ..Indent::default()
                },
                ..ParaProps::default()
            },
            vec![Run {
                text: text.to_string(),
                ..Run::default()
            }],
            None,
            &[TabStop {
                position_pt: 160.0,
                alignment: TabAlignment::Right,
            }],
        );
        let field_start = text.find('\t').unwrap() + 1;
        let actual = tab_aligned_position(&lines[0], field_start..text.len(), TabAlignment::Right);

        assert!((actual - 160.0).abs() <= 1.5, "actual={actual}");
    }

    #[test]
    fn paragraph_line_spacing_controls_layout_height() {
        let run = Run {
            text: "Line spacing".to_string(),
            ..Run::default()
        };
        let single = paragraph_line_metrics(
            ParaProps {
                spacing: Spacing {
                    line_pct: Some(1.0),
                    ..Spacing::default()
                },
                ..ParaProps::default()
            },
            vec![run.clone()],
        );
        let double = paragraph_line_metrics(
            ParaProps {
                spacing: Spacing {
                    line_pct: Some(2.0),
                    ..Spacing::default()
                },
                ..ParaProps::default()
            },
            vec![run],
        );

        assert_eq!(single.len(), 1);
        assert_eq!(double.len(), 1);
        assert!(
            double[0].0 > single[0].0 * 1.8,
            "double spacing should materially increase line height: single={} double={}",
            single[0].0,
            double[0].0
        );
    }

    #[test]
    fn explicit_zero_paragraph_after_spacing_suppresses_the_default_gap() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut paragraph = Paragraph {
            runs: vec![Run {
                text: "No trailing gap".to_string(),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        paragraph.props.spacing.after_pt = Some(0.0);
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();

        super::collect_blocks(
            &[Block::Paragraph(paragraph)],
            &mut flow,
            Geom::from_setup(&PageSetup::default()),
            &mut tcx,
            &mut capture,
        );

        assert!(!flow.iter().any(|item| matches!(item, FlowItem::Gap(_))));
    }

    #[test]
    fn paragraph_first_line_and_hanging_indents_affect_distinct_lines() {
        let text =
            "wrapped paragraph text that is deliberately long enough to occupy several lines";
        let run = Run {
            text: text.to_string(),
            ..Run::default()
        };
        let first_line = paragraph_line_metrics(
            ParaProps {
                indent: Indent {
                    left_pt: Some(12.0),
                    first_line_pt: Some(18.0),
                    ..Indent::default()
                },
                ..ParaProps::default()
            },
            vec![run.clone()],
        );
        let hanging = paragraph_line_metrics(
            ParaProps {
                indent: Indent {
                    left_pt: Some(30.0),
                    hanging_pt: Some(18.0),
                    ..Indent::default()
                },
                ..ParaProps::default()
            },
            vec![run],
        );

        assert!(first_line.len() >= 2);
        assert!(hanging.len() >= 2);
        assert!(
            first_line[0].1 > first_line[1].1 + 17.0,
            "first line should be indented independently: {first_line:?}"
        );
        assert!(
            hanging[0].1 + 17.0 < hanging[1].1,
            "hanging indent should move continuation lines inward: {hanging:?}"
        );
    }

    #[test]
    fn hidden_runs_are_excluded_from_render_layout() {
        let metrics = paragraph_line_metrics(
            ParaProps::default(),
            vec![
                Run {
                    text: "shown".to_string(),
                    ..Run::default()
                },
                Run {
                    text: "hidden".to_string(),
                    props: CharProps {
                        hidden: true,
                        ..CharProps::default()
                    },
                    ..Run::default()
                },
            ],
        );

        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].2, Some((0, "shown".chars().count())));
    }

    #[test]
    fn hidden_runs_preserve_source_offsets_for_visible_anchor_ranges() {
        let metrics = paragraph_line_metrics(
            ParaProps::default(),
            vec![
                Run {
                    text: "hidden".to_string(),
                    props: CharProps {
                        hidden: true,
                        ..CharProps::default()
                    },
                    ..Run::default()
                },
                Run {
                    text: "shown".to_string(),
                    ..Run::default()
                },
            ],
        );

        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].2, Some((6, 11)));
    }

    #[test]
    fn hidden_run_images_do_not_create_renderer_warnings() {
        let image = Image::default();
        let hidden = vec![Block::Paragraph(Paragraph {
            runs: vec![Run {
                image: Some(image.clone()),
                props: CharProps {
                    hidden: true,
                    ..CharProps::default()
                },
                ..Run::default()
            }],
            ..Paragraph::default()
        })];
        let visible = vec![Block::Paragraph(Paragraph {
            runs: vec![Run {
                image: Some(image),
                ..Run::default()
            }],
            ..Paragraph::default()
        })];

        assert_eq!(count_missing_image_bytes(&hidden), 0);
        assert_eq!(count_missing_image_bytes(&visible), 1);
    }

    fn laid_out_table_rows(table: &Table, geom: Geom) -> Vec<super::RowLayout> {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();
        layout_table(table, &mut flow, geom, &mut tcx, &mut capture);
        let FlowItem::Table { rows, .. } = flow.remove(0) else {
            panic!("table flow")
        };
        rows
    }

    fn laid_out_table_boxes(table: &Table, geom: Geom) -> Vec<(f32, f32)> {
        let mut rows = laid_out_table_rows(table, geom);
        rows.remove(0)
            .cells
            .into_iter()
            .map(|cell| (cell.x, cell.width))
            .collect()
    }

    fn table_box_bounds(boxes: &[(f32, f32)]) -> (f32, f32) {
        boxes.iter().fold(
            (f32::INFINITY, f32::NEG_INFINITY),
            |(left, right), (x, width)| (left.min(*x), right.max(*x + *width)),
        )
    }

    fn assert_table_boxes(actual: &[(f32, f32)], expected: &[(f32, f32)]) {
        assert_eq!(actual.len(), expected.len());
        for ((actual_x, actual_width), (expected_x, expected_width)) in actual.iter().zip(expected)
        {
            assert!(
                (actual_x - expected_x).abs() < 0.1,
                "x mismatch: actual={actual:?}, expected={expected:?}"
            );
            assert!(
                (actual_width - expected_width).abs() < 0.1,
                "width mismatch: actual={actual:?}, expected={expected:?}"
            );
        }
    }

    #[test]
    fn uniform_table_border_paint_resolves_color_and_bounded_eighth_point_width() {
        let default = super::table_border_paints(&Table::default());
        assert_eq!(default, super::TableBorderPaints::default());

        let color = Color::rgb(0x12, 0x67, 0xAB);
        let colored = super::table_border_paints(&Table {
            border_color: Some(color),
            ..Table::default()
        });
        for side in [
            TableBorderSide::Top,
            TableBorderSide::Left,
            TableBorderSide::Bottom,
            TableBorderSide::Right,
            TableBorderSide::InsideHorizontal,
            TableBorderSide::InsideVertical,
        ] {
            assert_eq!(
                colored.get(side).color,
                rgb::Color::new(color.r, color.g, color.b)
            );
            assert_eq!(colored.get(side).width, super::BORDER);
        }

        for (size, expected_width) in [
            (0, super::BORDER),
            (1, 0.125),
            (24, 3.0),
            (96, 12.0),
            (u16::MAX, 12.0),
        ] {
            let paint = super::table_border_paints(&Table {
                border_size_eighths: Some(size),
                ..Table::default()
            });
            for side in [
                TableBorderSide::Top,
                TableBorderSide::Left,
                TableBorderSide::Bottom,
                TableBorderSide::Right,
                TableBorderSide::InsideHorizontal,
                TableBorderSide::InsideVertical,
            ] {
                assert_eq!(paint.get(side).width, expected_width, "size={size}");
            }
        }
    }

    #[test]
    fn six_way_table_border_paint_resolves_overrides_and_fallbacks() {
        let sides = [
            TableBorderSide::Top,
            TableBorderSide::Left,
            TableBorderSide::Bottom,
            TableBorderSide::Right,
            TableBorderSide::InsideHorizontal,
            TableBorderSide::InsideVertical,
        ];
        let colors = [
            Color::rgb(0x10, 0x20, 0x30),
            Color::rgb(0x21, 0x31, 0x41),
            Color::rgb(0x32, 0x42, 0x52),
            Color::rgb(0x43, 0x53, 0x63),
            Color::rgb(0x54, 0x64, 0x74),
            Color::rgb(0x65, 0x75, 0x85),
        ];
        let sizes = [8, 16, 24, 32, 96, u16::MAX];
        let mut table = Table {
            border_color: Some(Color::rgb(0xAA, 0xBB, 0xCC)),
            border_size_eighths: Some(40),
            ..Table::default()
        };
        for ((side, color), size) in sides.into_iter().zip(colors).zip(sizes) {
            table.border_colors.set(side, color);
            table.border_sizes.set(side, size);
        }

        let paints = super::table_border_paints(&table);
        for ((side, color), expected_width) in sides
            .into_iter()
            .zip(colors)
            .zip([1.0, 2.0, 3.0, 4.0, 12.0, 12.0])
        {
            let paint = paints.get(side);
            assert_eq!(paint.color, rgb::Color::new(color.r, color.g, color.b));
            assert_eq!(paint.width, expected_width, "side={side:?}");
        }

        table.border_colors = Default::default();
        table.border_sizes = Default::default();
        table.border_sizes.left = Some(0);
        let fallback = super::table_border_paints(&table);
        for side in sides {
            assert_eq!(fallback.get(side).color, rgb::Color::new(0xAA, 0xBB, 0xCC));
            assert_eq!(fallback.get(side).width, 5.0);
        }
    }

    #[test]
    fn table_grid_assigns_six_physical_border_roles_in_ltr_and_rtl() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let table = Table {
            rows: vec![
                Row {
                    cells: vec![cell("A"), cell("B")],
                },
                Row {
                    cells: vec![cell("C"), cell("D")],
                },
            ],
            col_widths_pct: vec![0.5, 0.5],
            ..Table::default()
        };

        let rows = laid_out_table_rows(&table, geom);
        assert_eq!(
            rows[0].cells[0].border_edges,
            super::CellBorderEdges {
                top: Some(TableBorderSide::Top),
                left: Some(TableBorderSide::Left),
                bottom: Some(TableBorderSide::InsideHorizontal),
                right: Some(TableBorderSide::InsideVertical),
            }
        );
        assert_eq!(
            rows[1].cells[1].border_edges,
            super::CellBorderEdges {
                top: Some(TableBorderSide::InsideHorizontal),
                left: Some(TableBorderSide::InsideVertical),
                bottom: Some(TableBorderSide::Bottom),
                right: Some(TableBorderSide::Right),
            }
        );

        let mut rtl = table;
        rtl.bidi_visual = true;
        let rows = laid_out_table_rows(&rtl, geom);
        assert!(rows[0].cells[0].x > rows[0].cells[1].x);
        assert_eq!(
            rows[0].cells[0].border_edges,
            super::CellBorderEdges {
                top: Some(TableBorderSide::Top),
                left: Some(TableBorderSide::InsideVertical),
                bottom: Some(TableBorderSide::InsideHorizontal),
                right: Some(TableBorderSide::Right),
            }
        );
        assert_eq!(
            rows[0].cells[1].border_edges,
            super::CellBorderEdges {
                top: Some(TableBorderSide::Top),
                left: Some(TableBorderSide::Left),
                bottom: Some(TableBorderSide::InsideHorizontal),
                right: Some(TableBorderSide::InsideVertical),
            }
        );
    }

    #[test]
    fn table_spans_suppress_only_covered_inside_edges() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let rows = laid_out_table_rows(
            &Table {
                rows: vec![
                    Row {
                        cells: vec![
                            Cell {
                                row_span: 3,
                                col_span: 2,
                                blocks: vec![para("merged", None)],
                                ..Cell::default()
                            },
                            cell("A"),
                        ],
                    },
                    Row {
                        cells: vec![cell("B")],
                    },
                    Row {
                        cells: vec![cell("C")],
                    },
                ],
                col_widths_pct: vec![0.25, 0.25, 0.5],
                ..Table::default()
            },
            geom,
        );

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].cells[0].width, rows[1].cells[0].width);
        assert_eq!(
            rows[0].cells[0].border_edges,
            super::CellBorderEdges {
                top: Some(TableBorderSide::Top),
                left: Some(TableBorderSide::Left),
                bottom: None,
                right: Some(TableBorderSide::InsideVertical),
            }
        );
        assert_eq!(
            rows[1].cells[0].border_edges,
            super::CellBorderEdges {
                top: None,
                left: Some(TableBorderSide::Left),
                bottom: None,
                right: Some(TableBorderSide::InsideVertical),
            }
        );
        assert_eq!(
            rows[2].cells[0].border_edges,
            super::CellBorderEdges {
                top: None,
                left: Some(TableBorderSide::Left),
                bottom: Some(TableBorderSide::Bottom),
                right: Some(TableBorderSide::InsideVertical),
            }
        );
    }

    #[test]
    fn hostile_row_span_closes_on_the_last_real_row() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let rows = laid_out_table_rows(
            &Table {
                rows: vec![
                    Row {
                        cells: vec![Cell {
                            row_span: u16::MAX,
                            blocks: vec![para("bounded", None)],
                            ..Cell::default()
                        }],
                    },
                    Row::default(),
                ],
                ..Table::default()
            },
            geom,
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].cells[0].border_edges.bottom, None);
        assert_eq!(rows[1].cells[0].border_edges.top, None);
        assert_eq!(
            rows[1].cells[0].border_edges.bottom,
            Some(TableBorderSide::Bottom)
        );
    }

    #[test]
    fn table_border_rectangles_center_shared_edges_and_join_corners() {
        let rect = |x, y, w, h| super::BorderRect { x, y, w, h };
        let rects =
            super::table_border_rects(10.0, 20.0, 16.0, 24.0, 2.0).expect("positive finite cell");
        assert_eq!(
            rects,
            [
                rect(9.0, 19.0, 8.0, 2.0),
                rect(9.0, 23.0, 8.0, 2.0),
                rect(9.0, 19.0, 2.0, 6.0),
                rect(15.0, 19.0, 2.0, 6.0),
            ]
        );

        let left =
            super::table_border_rects(0.0, 0.0, 20.0, 12.0, 3.0).expect("left adjacent cell");
        let right =
            super::table_border_rects(20.0, 0.0, 40.0, 12.0, 3.0).expect("right adjacent cell");
        assert_eq!(
            left,
            [
                rect(-1.5, -1.5, 23.0, 3.0),
                rect(-1.5, 10.5, 23.0, 3.0),
                rect(-1.5, -1.5, 3.0, 15.0),
                rect(18.5, -1.5, 3.0, 15.0),
            ]
        );
        assert_eq!(left[3], right[2], "shared edge must overpaint once");
        assert!(super::table_border_rects(0.0, 0.0, 0.0, 4.0, 1.0).is_none());
        assert!(super::table_border_rects(0.0, 0.0, 4.0, 4.0, f32::NAN).is_none());
    }

    #[test]
    fn mixed_table_border_widths_extend_perpendicular_edges_exactly() {
        let paint = |width| {
            Some(super::TableBorderPaint {
                color: rgb::Color::black(),
                width,
            })
        };
        let rects = super::cell_border_rects(
            10.0,
            20.0,
            16.0,
            24.0,
            super::CellBorderPaints {
                top: paint(1.0),
                left: paint(2.0),
                bottom: paint(3.0),
                right: paint(4.0),
            },
        )
        .expect("finite mixed-width cell");

        assert_eq!(
            rects,
            [
                Some(super::BorderRect {
                    x: 9.0,
                    y: 19.5,
                    w: 9.0,
                    h: 1.0,
                }),
                Some(super::BorderRect {
                    x: 9.0,
                    y: 22.5,
                    w: 9.0,
                    h: 3.0,
                }),
                Some(super::BorderRect {
                    x: 9.0,
                    y: 19.5,
                    w: 2.0,
                    h: 6.0,
                }),
                Some(super::BorderRect {
                    x: 14.0,
                    y: 19.5,
                    w: 4.0,
                    h: 6.0,
                }),
            ]
        );
    }

    #[test]
    fn six_way_table_border_geometry_keeps_shared_edges_canonical() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut table = Table {
            rows: vec![
                Row {
                    cells: vec![cell("A"), cell("B")],
                },
                Row {
                    cells: vec![cell("C"), cell("D")],
                },
            ],
            ..Table::default()
        };
        for (side, size) in [
            (TableBorderSide::Top, 8),
            (TableBorderSide::Left, 16),
            (TableBorderSide::Bottom, 24),
            (TableBorderSide::Right, 32),
            (TableBorderSide::InsideHorizontal, 40),
            (TableBorderSide::InsideVertical, 48),
        ] {
            table.border_sizes.set(side, size);
        }
        let rows = laid_out_table_rows(&table, geom);
        let upper_left = &rows[0].cells[0];
        let upper_right = &rows[0].cells[1];
        let lower_left = &rows[1].cells[0];
        let upper_left_rects = super::cell_border_rects(
            upper_left.x,
            0.0,
            upper_left.right,
            rows[0].height,
            super::CellBorderPaints::resolve(upper_left.border_edges, rows[0].border),
        )
        .expect("upper-left cell");
        let upper_right_rects = super::cell_border_rects(
            upper_right.x,
            0.0,
            upper_right.right,
            rows[0].height,
            super::CellBorderPaints::resolve(upper_right.border_edges, rows[0].border),
        )
        .expect("upper-right cell");
        let lower_left_rects = super::cell_border_rects(
            lower_left.x,
            rows[0].height,
            lower_left.right,
            rows[0].height + rows[1].height,
            super::CellBorderPaints::resolve(lower_left.border_edges, rows[1].border),
        )
        .expect("lower-left cell");

        assert_eq!(
            upper_left_rects[3], upper_right_rects[2],
            "insideV must overpaint at one canonical rectangle"
        );
        assert_eq!(
            upper_left_rects[1], lower_left_rects[0],
            "insideH must overpaint at one canonical rectangle"
        );
    }

    #[test]
    fn vertical_t_junction_precedence_is_topology_and_bidi_stable() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let split_row = || Row {
            cells: vec![cell("A"), cell("B")],
        };
        let spanning_row = || Row {
            cells: vec![Cell {
                col_span: 2,
                blocks: vec![para("span", None)],
                ..Cell::default()
            }],
        };

        for bidi_visual in [false, true] {
            let mut terminating = Table {
                rows: vec![split_row(), spanning_row()],
                bidi_visual,
                ..Table::default()
            };
            terminating.border_colors.set(
                TableBorderSide::InsideHorizontal,
                Color::rgb(0xC4, 0x21, 0x32),
            );
            terminating.border_colors.set(
                TableBorderSide::InsideVertical,
                Color::rgb(0x16, 0x5D, 0xA8),
            );
            terminating
                .border_sizes
                .set(TableBorderSide::InsideHorizontal, 16);
            terminating
                .border_sizes
                .set(TableBorderSide::InsideVertical, 24);
            let rows = laid_out_table_rows(&terminating, geom);
            let previous = super::row_vertical_border_lines(&rows[0], 0.0);
            let current = super::row_vertical_border_lines(&rows[1], 0.0);
            let junctions = super::terminal_vertical_junctions(&previous, &rows[1], &current, 0.0);

            assert_eq!(junctions.len(), 1, "bidi_visual={bidi_visual}");
            assert_eq!(
                junctions[0].0.paint, rows[0].border.inside_v,
                "bidi_visual={bidi_visual}"
            );
            assert_eq!(
                junctions[0].1, rows[1].border.inside_h.width,
                "bidi_visual={bidi_visual}"
            );

            let continuing = Table {
                rows: vec![spanning_row(), split_row()],
                ..terminating.clone()
            };
            let rows = laid_out_table_rows(&continuing, geom);
            let previous = super::row_vertical_border_lines(&rows[0], 0.0);
            let current = super::row_vertical_border_lines(&rows[1], 0.0);
            assert!(
                super::terminal_vertical_junctions(&previous, &rows[1], &current, 0.0).is_empty(),
                "the current row paints its own vertical last; bidi_visual={bidi_visual}"
            );
            assert!(
                current
                    .iter()
                    .any(|line| line.paint == rows[1].border.inside_v),
                "bidi_visual={bidi_visual}"
            );

            let uniform = Table {
                rows: vec![split_row(), spanning_row()],
                bidi_visual,
                ..Table::default()
            };
            let rows = laid_out_table_rows(&uniform, geom);
            let previous = super::row_vertical_border_lines(&rows[0], 0.0);
            let current = super::row_vertical_border_lines(&rows[1], 0.0);
            assert!(
                super::terminal_vertical_junctions(&previous, &rows[1], &current, 0.0).is_empty(),
                "equal paints need no compatibility-changing overlay"
            );
        }
    }

    #[test]
    fn table_border_width_is_uniformly_bounded_across_asymmetric_cells_and_rows() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let rows = laid_out_table_rows(
            &Table {
                rows: vec![
                    Row {
                        cells: vec![cell("A"), cell("B")],
                    },
                    Row {
                        cells: vec![cell("C"), cell("one\ntwo\nthree\nfour")],
                    },
                ],
                col_widths_pct: vec![0.04, 0.96],
                border_size_eighths: Some(96),
                ..Table::default()
            },
            geom,
        );
        assert!(rows[1].height > rows[0].height);
        let bounded_width = rows
            .iter()
            .flat_map(|row| {
                row.cells
                    .iter()
                    .map(move |cell| cell.width.min(row.height) * 0.5)
            })
            .fold(12.0_f32, f32::min);
        assert!(bounded_width < 12.0);
        assert!(rows.iter().all(|row| {
            [
                row.border.top,
                row.border.left,
                row.border.bottom,
                row.border.right,
                row.border.inside_h,
                row.border.inside_v,
            ]
            .into_iter()
            .all(|paint| (paint.width - bounded_width).abs() < f32::EPSILON)
        }));

        let upper_left = &rows[0].cells[0];
        let upper_right = &rows[0].cells[1];
        let left_rects = super::table_border_rects(
            upper_left.x,
            0.0,
            upper_left.right,
            rows[0].height,
            rows[0].border.top.width,
        )
        .expect("upper-left cell");
        let right_rects = super::table_border_rects(
            upper_right.x,
            0.0,
            upper_right.right,
            rows[0].height,
            rows[0].border.top.width,
        )
        .expect("upper-right cell");
        assert_eq!(left_rects[3], right_rects[2]);

        let lower_right = &rows[1].cells[1];
        let lower_rects = super::table_border_rects(
            lower_right.x,
            rows[0].height,
            lower_right.right,
            rows[0].height + rows[1].height,
            rows[1].border.top.width,
        )
        .expect("lower-right cell");
        assert_eq!(right_rects[1], lower_rects[0]);
    }

    #[test]
    fn six_way_table_border_widths_share_the_table_geometry_bound() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut table = Table {
            rows: vec![
                Row {
                    cells: vec![cell("A"), cell("B")],
                },
                Row {
                    cells: vec![cell("C"), cell("one\ntwo\nthree\nfour")],
                },
            ],
            col_widths_pct: vec![0.04, 0.96],
            ..Table::default()
        };
        let cases = [
            (TableBorderSide::Top, 8, 1.0_f32),
            (TableBorderSide::Left, 16, 2.0),
            (TableBorderSide::Bottom, 24, 3.0),
            (TableBorderSide::Right, 32, 4.0),
            (TableBorderSide::InsideHorizontal, 64, 8.0),
            (TableBorderSide::InsideVertical, u16::MAX, 12.0),
        ];
        for (side, size, _) in cases {
            table.border_sizes.set(side, size);
        }
        let rows = laid_out_table_rows(&table, geom);
        let max_width = rows
            .iter()
            .flat_map(|row| {
                row.cells
                    .iter()
                    .map(move |cell| cell.width.min(row.height) * 0.5)
            })
            .fold(f32::INFINITY, f32::min);

        assert!(max_width < 12.0);
        for (side, _, authored_width) in cases {
            assert!(rows.iter().all(|row| {
                (row.border.get(side).width - authored_width.min(max_width)).abs() < f32::EPSILON
            }));
        }
    }

    #[test]
    fn table_border_shared_edges_use_canonical_coordinates_in_offset_tables() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        for bidi_visual in [false, true] {
            let mut rows = laid_out_table_rows(
                &Table {
                    rows: vec![Row {
                        cells: vec![cell("A"), cell("B"), cell("C"), cell("D")],
                    }],
                    col_widths_pct: vec![0.1, 0.2, 0.3, 0.4],
                    width_pct: Some(0.73),
                    align: Some(Align::Center),
                    bidi_visual,
                    border_size_eighths: Some(24),
                    ..Table::default()
                },
                geom,
            );
            let row = rows.remove(0);
            let mut cells = row.cells.iter().collect::<Vec<_>>();
            cells.sort_by(|left, right| left.x.total_cmp(&right.x));
            for pair in cells.windows(2) {
                let [left, right] = pair else { unreachable!() };
                assert_eq!(left.right, right.x, "bidi_visual={bidi_visual}");
                let left_rects = super::table_border_rects(
                    left.x,
                    0.0,
                    left.right,
                    row.height,
                    row.border.inside_v.width,
                )
                .expect("left offset cell");
                let right_rects = super::table_border_rects(
                    right.x,
                    0.0,
                    right.right,
                    row.height,
                    row.border.inside_v.width,
                )
                .expect("right offset cell");
                assert_eq!(left_rects[3], right_rects[2], "bidi_visual={bidi_visual}");
            }
        }
    }

    #[test]
    fn preferred_table_width_alignment_indent_and_bidi_define_local_box() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let base = Table {
            rows: vec![Row {
                cells: vec![cell("A"), cell("B")],
            }],
            col_widths_pct: vec![0.25, 0.75],
            width_pct: Some(0.5),
            ..Table::default()
        };
        let cases = [
            (
                "ltr center ignores indent",
                Some(Align::Center),
                false,
                Some(720),
                vec![(45.0, 22.5), (67.5, 67.5)],
            ),
            (
                "ltr trailing ignores indent",
                Some(Align::Right),
                false,
                Some(720),
                vec![(90.0, 22.5), (112.5, 67.5)],
            ),
            (
                "ltr leading indent",
                Some(Align::Left),
                false,
                Some(720),
                vec![(36.0, 22.5), (58.5, 67.5)],
            ),
            (
                "ltr default leading indent",
                None,
                false,
                Some(720),
                vec![(36.0, 22.5), (58.5, 67.5)],
            ),
            (
                "rtl center ignores indent",
                Some(Align::Center),
                true,
                Some(720),
                vec![(112.5, 22.5), (45.0, 67.5)],
            ),
            (
                "rtl leading indent and local mirror",
                Some(Align::Left),
                true,
                Some(720),
                vec![(121.5, 22.5), (54.0, 67.5)],
            ),
            (
                "rtl trailing ignores indent",
                Some(Align::Right),
                true,
                Some(720),
                vec![(67.5, 22.5), (0.0, 67.5)],
            ),
        ];

        for (name, align, bidi_visual, indent_twips, expected) in cases {
            let boxes = laid_out_table_boxes(
                &Table {
                    align,
                    bidi_visual,
                    indent_twips,
                    ..base.clone()
                },
                geom,
            );
            assert_table_boxes(&boxes, &expected);
            assert!(
                (table_box_bounds(&boxes).1 - table_box_bounds(&boxes).0 - 90.0).abs() < 0.1,
                "{name}: boxes={boxes:?}"
            );
        }
    }

    #[test]
    fn preferred_table_width_is_column_relative_and_malformed_values_are_bounded() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let base = Table {
            rows: vec![Row {
                cells: vec![cell("A"), cell("B")],
            }],
            col_widths_pct: vec![0.5, 0.5],
            align: Some(Align::Center),
            ..Table::default()
        };

        let column_boxes = laid_out_table_boxes(
            &Table {
                width_pct: Some(0.5),
                ..base.clone()
            },
            geom.with_content_width(81.0),
        );
        assert_table_boxes(&column_boxes, &[(20.25, 20.25), (40.5, 20.25)]);

        for width_pct in [
            None,
            Some(0.0),
            Some(-0.5),
            Some(f32::NAN),
            Some(f32::NEG_INFINITY),
            Some(f32::INFINITY),
            Some(2.0),
        ] {
            let boxes = laid_out_table_boxes(
                &Table {
                    width_pct,
                    ..base.clone()
                },
                geom,
            );
            assert_table_boxes(&boxes, &[(0.0, 90.0), (90.0, 90.0)]);
        }

        let bounded_indent = laid_out_table_boxes(
            &Table {
                width_pct: Some(0.5),
                align: Some(Align::Left),
                indent_twips: Some(i32::MAX),
                ..base.clone()
            },
            geom,
        );
        assert_table_boxes(&bounded_indent, &[(90.0, 45.0), (135.0, 45.0)]);

        let negative_indent = laid_out_table_boxes(
            &Table {
                width_pct: Some(0.5),
                align: Some(Align::Left),
                indent_twips: Some(-720),
                ..base.clone()
            },
            geom,
        );
        assert_table_boxes(&negative_indent, &[(0.0, 45.0), (45.0, 45.0)]);

        let justify_fallback = laid_out_table_boxes(
            &Table {
                width_pct: Some(0.5),
                align: Some(Align::Justify),
                indent_twips: Some(720),
                ..base.clone()
            },
            geom,
        );
        assert_table_boxes(&justify_fallback, &[(0.0, 45.0), (45.0, 45.0)]);
        let rtl_justify_fallback = laid_out_table_boxes(
            &Table {
                width_pct: Some(0.5),
                align: Some(Align::Justify),
                bidi_visual: true,
                indent_twips: Some(720),
                ..base.clone()
            },
            geom,
        );
        assert_table_boxes(&rtl_justify_fallback, &[(135.0, 45.0), (90.0, 45.0)]);

        let malformed_columns = laid_out_table_boxes(
            &Table {
                width_pct: Some(0.5),
                col_widths_pct: vec![f32::INFINITY, 1.0],
                ..base.clone()
            },
            geom,
        );
        assert!(malformed_columns
            .iter()
            .all(|(x, width)| x.is_finite() && width.is_finite() && *width > 0.0));
        assert!((table_box_bounds(&malformed_columns).0 - 45.0).abs() < 0.1);
        assert!((table_box_bounds(&malformed_columns).1 - 135.0).abs() < 0.1);

        let underflowing_columns = laid_out_table_boxes(
            &Table {
                width_pct: Some(0.5),
                col_widths_pct: vec![f32::MIN_POSITIVE, f32::MAX],
                ..base
            },
            geom,
        );
        assert!(underflowing_columns
            .iter()
            .all(|(x, width)| x.is_finite() && width.is_finite() && *width > 0.0));
        assert!((table_box_bounds(&underflowing_columns).0 - 45.0).abs() < 0.1);
        assert!((table_box_bounds(&underflowing_columns).1 - 135.0).abs() < 0.1);
    }

    #[test]
    fn table_box_survives_header_repetition_and_row_splitting() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let tall_blocks = (0..12)
            .map(|index| para(&format!("line {index}"), None))
            .collect();
        let table = Table {
            rows: vec![
                Row {
                    cells: vec![cell("Header A"), cell("Header B")],
                },
                Row {
                    cells: vec![
                        Cell {
                            blocks: tall_blocks,
                            ..Cell::default()
                        },
                        cell("Body"),
                    ],
                },
            ],
            header_rows: 1,
            col_widths_pct: vec![0.5, 0.5],
            width_pct: Some(0.5),
            align: Some(Align::Left),
            indent_twips: Some(400),
            ..Table::default()
        };
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();
        layout_table(&table, &mut flow, geom, &mut tcx, &mut capture);
        let pagination = paginate(flow, geom, &SectionSetup::default());
        assert!(pagination.pages.len() > 1);
        let row_boxes = pagination
            .pages
            .iter()
            .flatten()
            .filter_map(|placed| match &placed.item {
                FlowItem::Row(row) => Some(
                    row.cells
                        .iter()
                        .map(|cell| (cell.x, cell.width))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(row_boxes.len() > table.rows.len());
        for boxes in row_boxes {
            assert_table_boxes(&boxes, &[(20.0, 45.0), (65.0, 45.0)]);
        }
    }

    #[test]
    fn table_cell_paragraphs_use_line_spacing() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let cell = |line_pct| Cell {
            blocks: vec![Block::Paragraph(Paragraph {
                props: ParaProps {
                    spacing: Spacing {
                        line_pct: Some(line_pct),
                        ..Spacing::default()
                    },
                    indent: Indent {
                        left_pt: Some(12.0),
                        ..Indent::default()
                    },
                    ..ParaProps::default()
                },
                runs: vec![Run {
                    text: "cell text".to_string(),
                    ..Run::default()
                }],
            })],
            ..Cell::default()
        };
        let mut capture = LayoutCapture::default();
        let single = shape_cell(&cell(1.0), 160.0, 0, &mut tcx, &mut capture);
        let double = shape_cell(&cell(2.0), 160.0, 0, &mut tcx, &mut capture);

        assert_eq!(single.len(), 1);
        assert_eq!(double.len(), 1);
        assert!(double[0].height > single[0].height * 1.8);
        assert!((single[0].x_indent - 12.0).abs() < 0.1);
        assert!(
            (cell_line_origin(100.0, cell_insets(None, 160.0), &single[0]) - 115.0).abs() < 0.1
        );
    }

    #[test]
    fn table_cell_explicit_paragraph_spacing_expands_row_not_line_box() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut row_for = |before_pt, after_pt| {
            let table = Table {
                rows: vec![Row {
                    cells: vec![Cell {
                        blocks: vec![Block::Paragraph(Paragraph {
                            props: ParaProps {
                                spacing: Spacing {
                                    before_pt,
                                    after_pt,
                                    ..Spacing::default()
                                },
                                shading: Some(Color::rgb(0xEE, 0xF1, 0xF4)),
                                ..ParaProps::default()
                            },
                            runs: vec![Run {
                                text: "cell text".to_string(),
                                ..Run::default()
                            }],
                        })],
                        ..Cell::default()
                    }],
                }],
                ..Table::default()
            };
            let mut flow = Vec::new();
            let mut capture = LayoutCapture::default();
            layout_table(&table, &mut flow, geom, &mut tcx, &mut capture);
            let FlowItem::Table { mut rows, .. } = flow.remove(0) else {
                panic!("table flow item")
            };
            rows.remove(0)
        };

        let compact = row_for(None, None);
        let spaced = row_for(Some(11.0), Some(7.0));
        let compact_line = &compact.cells[0].lines[0];
        let spaced_line = &spaced.cells[0].lines[0];

        assert_close(spaced_line.height, compact_line.height);
        assert_close(spaced_line.baseline, compact_line.baseline);
        assert_eq!(spaced_line.background, compact_line.background);
        assert_close(spaced_line.cell_spacing.before, 11.0);
        assert_close(spaced_line.cell_spacing.after, 7.0);
        assert_close(spaced_line.cell_extent() - spaced_line.height, 18.0);
        assert_close(spaced.height - compact.height, 18.0);
    }

    #[test]
    fn table_cell_spacing_attaches_to_true_paragraph_edges_and_rejects_invalid_values() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let paragraph = |text: &str, spacing: Spacing| Paragraph {
            props: ParaProps {
                spacing,
                ..ParaProps::default()
            },
            runs: vec![Run {
                text: text.to_string(),
                ..Run::default()
            }],
        };
        let mut capture = LayoutCapture::default();
        let lines = shape_cell(
            &Cell {
                blocks: vec![Block::Paragraph(paragraph(
                    "one\ntwo\nthree",
                    Spacing {
                        before_pt: Some(9.0),
                        after_pt: Some(4.0),
                        ..Spacing::default()
                    },
                ))],
                ..Cell::default()
            },
            160.0,
            0,
            &mut tcx,
            &mut capture,
        );

        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines
                .iter()
                .map(|line| (line.cell_spacing.before, line.cell_spacing.after))
                .collect::<Vec<_>>(),
            [(9.0, 0.0), (0.0, 0.0), (0.0, 4.0)]
        );

        for spacing in [
            Spacing::default(),
            Spacing {
                before_pt: Some(0.0),
                after_pt: Some(-1.0),
                ..Spacing::default()
            },
            Spacing {
                before_pt: Some(f32::NAN),
                after_pt: Some(f32::INFINITY),
                ..Spacing::default()
            },
        ] {
            let mut capture = LayoutCapture::default();
            let invalid = shape_cell(
                &Cell {
                    blocks: vec![Block::Paragraph(paragraph("bounded", spacing))],
                    ..Cell::default()
                },
                160.0,
                0,
                &mut tcx,
                &mut capture,
            );
            assert_eq!(invalid.len(), 1);
            assert_eq!(invalid[0].cell_spacing, super::CellLineSpacing::default());
        }

        let mut truncated = lines;
        super::truncate_cell_paragraph_lines(
            &mut truncated,
            2,
            Spacing {
                before_pt: Some(9.0),
                after_pt: Some(4.0),
                ..Spacing::default()
            },
        );
        assert_eq!(truncated.len(), 2);
        assert_close(truncated[0].cell_spacing.before, 9.0);
        assert_close(truncated[1].cell_spacing.after, 0.0);
    }

    #[test]
    fn max_cell_line_truncation_omits_nonfinal_trailing_spacing() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let text = std::iter::repeat_n("line", super::MAX_CELL_LINES + 1)
            .collect::<Vec<_>>()
            .join("\n");
        let mut capture = LayoutCapture::default();
        let lines = shape_cell(
            &Cell {
                blocks: vec![Block::Paragraph(Paragraph {
                    props: ParaProps {
                        spacing: Spacing {
                            before_pt: Some(3.0),
                            after_pt: Some(5.0),
                            ..Spacing::default()
                        },
                        ..ParaProps::default()
                    },
                    runs: vec![Run {
                        text,
                        ..Run::default()
                    }],
                })],
                ..Cell::default()
            },
            160.0,
            0,
            &mut tcx,
            &mut capture,
        );

        assert_eq!(lines.len(), super::MAX_CELL_LINES);
        assert_close(lines[0].cell_spacing.before, 3.0);
        assert_close(lines.last().unwrap().cell_spacing.after, 0.0);
    }

    #[test]
    fn table_cell_spacing_follows_direct_and_nested_source_order_without_default_gap() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let paragraph = |text: &str, before_pt, after_pt| {
            Block::Paragraph(Paragraph {
                props: ParaProps {
                    spacing: Spacing {
                        before_pt,
                        after_pt,
                        ..Spacing::default()
                    },
                    ..ParaProps::default()
                },
                runs: vec![Run {
                    text: text.to_string(),
                    ..Run::default()
                }],
            })
        };
        let cell = Cell {
            blocks: vec![
                paragraph("direct", Some(2.0), Some(3.0)),
                Block::Table(Table {
                    rows: vec![Row {
                        cells: vec![Cell {
                            blocks: vec![paragraph("nested", Some(5.0), Some(7.0))],
                            ..Cell::default()
                        }],
                    }],
                    ..Table::default()
                }),
                paragraph("tail", None, None),
            ],
            ..Cell::default()
        };
        let mut capture = LayoutCapture::default();
        let lines = shape_cell(&cell, 160.0, 0, &mut tcx, &mut capture);

        assert_eq!(
            lines.iter().map(shaped_line_text).collect::<Vec<_>>(),
            ["direct", "nested", "tail"]
        );
        assert_eq!(
            lines
                .iter()
                .map(|line| (line.cell_spacing.before, line.cell_spacing.after))
                .collect::<Vec<_>>(),
            [(2.0, 3.0), (5.0, 7.0), (0.0, 0.0)]
        );
        assert_close(
            super::cell_lines_extent(&lines) - lines.iter().map(|line| line.height).sum::<f32>(),
            17.0,
        );
    }

    #[test]
    fn table_cell_vertical_alignment_uses_full_spacing_extent() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut rows = laid_out_table_rows(
            &Table {
                rows: vec![Row {
                    cells: vec![Cell {
                        blocks: vec![Block::Paragraph(Paragraph {
                            props: ParaProps {
                                spacing: Spacing {
                                    before_pt: Some(6.0),
                                    after_pt: Some(4.0),
                                    ..Spacing::default()
                                },
                                ..ParaProps::default()
                            },
                            runs: vec![Run {
                                text: "aligned".to_string(),
                                ..Run::default()
                            }],
                        })],
                        ..Cell::default()
                    }],
                }],
                ..Table::default()
            },
            geom,
        );
        let mut cell = rows.remove(0).cells.remove(0);
        let row_height =
            cell.insets.top + super::cell_lines_extent(&cell.lines) + cell.insets.bottom + 40.0;
        let before = cell.lines[0].cell_spacing.before;

        cell.valign = VCell::Top;
        assert_close(super::cell_vertical_offset(&cell, row_height), 0.0);
        cell.valign = VCell::Center;
        assert_close(super::cell_vertical_offset(&cell, row_height), 20.0);
        assert_close(
            100.0 + cell.insets.top + super::cell_vertical_offset(&cell, row_height) + before,
            100.0 + cell.insets.top + 20.0 + 6.0,
        );
        cell.valign = VCell::Bottom;
        assert_close(super::cell_vertical_offset(&cell, row_height), 40.0);
    }

    #[test]
    fn bidi_visual_table_mirrors_logical_cell_positions() {
        let fonts = vec![
            rwml_fonts::noto_sans_kr_subset_with_hanja().to_vec(),
            rwml_fonts::noto_sans_arabic_subset().to_vec(),
            rwml_fonts::noto_sans_hebrew_subset().to_vec(),
        ];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();
        layout_table(
            &Table {
                rows: vec![Row {
                    cells: vec![
                        Cell {
                            blocks: vec![Block::Paragraph(Paragraph {
                                runs: vec![Run {
                                    text: "خلية أولى 123".to_string(),
                                    props: CharProps {
                                        rtl: true,
                                        ..CharProps::default()
                                    },
                                    ..Run::default()
                                }],
                                props: ParaProps {
                                    align: Align::Right,
                                    bidi: true,
                                    ..ParaProps::default()
                                },
                            })],
                            margins: Some(CellMargins {
                                top: 40,
                                right: 40,
                                bottom: 60,
                                left: 200,
                            }),
                            ..Cell::default()
                        },
                        Cell {
                            blocks: vec![Block::Paragraph(Paragraph {
                                runs: vec![Run {
                                    text: "תא שני 456".to_string(),
                                    props: CharProps {
                                        rtl: true,
                                        ..CharProps::default()
                                    },
                                    ..Run::default()
                                }],
                                props: ParaProps {
                                    align: Align::Right,
                                    bidi: true,
                                    ..ParaProps::default()
                                },
                            })],
                            margins: Some(CellMargins {
                                top: 80,
                                right: 240,
                                bottom: 20,
                                left: 60,
                            }),
                            ..Cell::default()
                        },
                    ],
                }],
                col_widths_pct: vec![25.0, 75.0],
                bidi_visual: true,
                ..Table::default()
            },
            &mut flow,
            geom,
            &mut tcx,
            &mut capture,
        );

        let FlowItem::Table { rows, .. } = &flow[0] else {
            panic!("table flow")
        };
        let cells = &rows[0].cells;
        assert_eq!(cells.len(), 2);
        assert!((cells[0].x - 135.0).abs() < 0.1, "cells={:?}", cells[0].x);
        assert!((cells[1].x - 0.0).abs() < 0.1, "cells={:?}", cells[1].x);
        assert!((cells[0].width - 45.0).abs() < 0.1);
        assert!((cells[1].width - 135.0).abs() < 0.1);
        assert!((cells[0].insets.left - 10.0).abs() < 0.1);
        assert!((cells[0].insets.right - 2.0).abs() < 0.1);
        assert!((cells[1].insets.left - 3.0).abs() < 0.1);
        assert!((cells[1].insets.right - 12.0).abs() < 0.1);
        assert!(cells[0]
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .any(|run| run.text.contains("خلية أولى 123")));
        assert!(cells[1]
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .any(|run| run.text.contains("תא שני 456")));
        assert!(
            (cell_line_origin(cells[0].x, cells[0].insets, &cells[0].lines[0]) - 145.0).abs() < 0.1
        );
        assert!(
            (cell_line_origin(cells[1].x, cells[1].insets, &cells[1].lines[0]) - 3.0).abs() < 0.1
        );
    }

    #[test]
    fn table_cell_margins_control_content_origin_and_row_height() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let table = Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: vec![Block::Paragraph(Paragraph {
                        props: ParaProps::default(),
                        runs: vec![Run {
                            text: "Inset".to_string(),
                            ..Run::default()
                        }],
                    })],
                    margins: Some(CellMargins {
                        top: 400,
                        right: 720,
                        bottom: 400,
                        left: 720,
                    }),
                    ..Cell::default()
                }],
            }],
            ..Table::default()
        };
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();
        layout_table(&table, &mut flow, geom, &mut tcx, &mut capture);
        let FlowItem::Table { rows, .. } = flow.remove(0) else {
            panic!("table flow item")
        };
        let row = &rows[0];
        let cell = &row.cells[0];

        assert!(cell_line_origin(cell.x, cell.insets, &cell.lines[0]) - cell.x >= 36.0);
        assert!(row.height >= cell.lines[0].height + 40.0);
    }

    #[test]
    fn split_table_cell_keeps_outer_margins_on_outer_fragments_only() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let table = Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: vec![
                        Block::Paragraph(Paragraph {
                            props: ParaProps::default(),
                            runs: vec![Run {
                                text: "First line".to_string(),
                                ..Run::default()
                            }],
                        }),
                        Block::Paragraph(Paragraph {
                            props: ParaProps::default(),
                            runs: vec![Run {
                                text: "Second line".to_string(),
                                ..Run::default()
                            }],
                        }),
                    ],
                    margins: Some(CellMargins {
                        top: 400,
                        bottom: 600,
                        ..CellMargins::default()
                    }),
                    ..Cell::default()
                }],
            }],
            border_color: Some(Color::rgb(0x31, 0x75, 0x9B)),
            border_size_eighths: Some(24),
            ..Table::default()
        };
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();
        layout_table(&table, &mut flow, geom, &mut tcx, &mut capture);
        let FlowItem::Table { mut rows, .. } = flow.remove(0) else {
            panic!("table flow item")
        };
        let first_line_height = rows[0].cells[0].lines[0].height;
        let (head, tail) = split_row(rows.remove(0), first_line_height + 50.0);
        let tail = tail.expect("second line remains");

        assert_eq!(head.cells[0].insets.top, 20.0);
        assert_eq!(head.cells[0].insets.bottom, 0.0);
        assert_eq!(tail.cells[0].insets.top, 0.0);
        assert_eq!(tail.cells[0].insets.bottom, 30.0);
        let expected_border = super::table_border_paints(&table);
        assert_eq!(head.border, expected_border);
        assert_eq!(tail.border, expected_border);
        assert_eq!(
            head.cells[0].border_edges,
            super::CellBorderEdges {
                top: Some(TableBorderSide::Top),
                left: Some(TableBorderSide::Left),
                bottom: None,
                right: Some(TableBorderSide::Right),
            }
        );
        assert_eq!(
            tail.cells[0].border_edges,
            super::CellBorderEdges {
                top: None,
                left: Some(TableBorderSide::Left),
                bottom: Some(TableBorderSide::Bottom),
                right: Some(TableBorderSide::Right),
            }
        );
    }

    #[test]
    fn split_table_cell_spacing_stays_on_true_outer_lines() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let table = Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: vec![Block::Paragraph(Paragraph {
                        props: ParaProps {
                            spacing: Spacing {
                                before_pt: Some(8.0),
                                after_pt: Some(6.0),
                                ..Spacing::default()
                            },
                            ..ParaProps::default()
                        },
                        runs: vec![Run {
                            text: "first\nmiddle\nlast".to_string(),
                            ..Run::default()
                        }],
                    })],
                    ..Cell::default()
                }],
            }],
            ..Table::default()
        };
        let mut rows = laid_out_table_rows(&table, geom);
        let row = rows.remove(0);
        let original_extent = super::cell_lines_extent(&row.cells[0].lines);
        assert_eq!(row.cells[0].lines.len(), 3);
        let first_budget = row.cells[0].insets.top
            + row.cells[0].insets.bottom
            + row.cells[0].lines[0].cell_extent();

        let (first, rest) = split_row(row, first_budget);
        let rest = rest.expect("two lines remain");
        let middle_budget = rest.cells[0].insets.top
            + rest.cells[0].insets.bottom
            + rest.cells[0].lines[0].cell_extent();
        let (middle, last) = split_row(rest, middle_budget);
        let last = last.expect("final line remains");

        assert_eq!(first.cells[0].lines.len(), 1);
        assert_eq!(middle.cells[0].lines.len(), 1);
        assert_eq!(last.cells[0].lines.len(), 1);
        assert_eq!(
            first.cells[0].lines[0].cell_spacing,
            super::CellLineSpacing {
                before: 8.0,
                after: 0.0
            }
        );
        assert_eq!(
            middle.cells[0].lines[0].cell_spacing,
            super::CellLineSpacing::default()
        );
        assert_eq!(
            last.cells[0].lines[0].cell_spacing,
            super::CellLineSpacing {
                before: 0.0,
                after: 6.0
            }
        );
        assert_close(
            super::cell_lines_extent(&first.cells[0].lines)
                + super::cell_lines_extent(&middle.cells[0].lines)
                + super::cell_lines_extent(&last.cells[0].lines),
            original_extent,
        );
    }

    fn cell_row_with_pagination(paragraphs: &[(&str, PaginationHint)]) -> super::RowLayout {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let table = Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: paragraphs
                        .iter()
                        .map(|(text, _)| {
                            Block::Paragraph(Paragraph {
                                runs: vec![Run {
                                    text: (*text).to_string(),
                                    ..Run::default()
                                }],
                                ..Paragraph::default()
                            })
                        })
                        .collect(),
                    ..Cell::default()
                }],
            }],
            ..Table::default()
        };
        let cell_pagination = vec![vec![paragraphs
            .iter()
            .map(|(_, hint)| Some(*hint))
            .collect::<Vec<_>>()]];
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();
        layout_table_with_row_pagination(
            &table,
            &mut flow,
            geom,
            &mut tcx,
            &mut capture,
            TablePaginationView {
                cells: Some(&cell_pagination),
                ..TablePaginationView::default()
            },
        );
        let FlowItem::Table { mut rows, .. } = flow.remove(0) else {
            panic!("table flow item")
        };
        rows.remove(0)
    }

    fn nested_cell_row_with_pagination(
        nested_cells: &[Vec<(&str, PaginationHint)>],
    ) -> super::RowLayout {
        nested_cell_row_with_pagination_and_row_policy(nested_cells, false)
    }

    fn nested_cell_row_with_pagination_and_row_policy(
        nested_cells: &[Vec<(&str, PaginationHint)>],
        cant_split: bool,
    ) -> super::RowLayout {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let nested_row = Row {
            cells: nested_cells
                .iter()
                .map(|paragraphs| Cell {
                    blocks: paragraphs
                        .iter()
                        .map(|(text, _)| {
                            Block::Paragraph(Paragraph {
                                runs: vec![Run {
                                    text: (*text).to_string(),
                                    ..Run::default()
                                }],
                                ..Paragraph::default()
                            })
                        })
                        .collect(),
                    ..Cell::default()
                })
                .collect(),
        };
        let table = Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: vec![Block::Table(Table {
                        rows: vec![nested_row],
                        ..Table::default()
                    })],
                    ..Cell::default()
                }],
            }],
            ..Table::default()
        };
        let cell_pagination = vec![vec![vec![None]]];
        let nested_pagination = vec![vec![vec![Some(TablePaginationHints {
            rows: vec![TableRowPaginationHint { cant_split }],
            cells: vec![nested_cells
                .iter()
                .map(|paragraphs| {
                    paragraphs
                        .iter()
                        .map(|(_, hint)| Some(*hint))
                        .collect::<Vec<_>>()
                })
                .collect()],
            nested: vec![nested_cells
                .iter()
                .map(|paragraphs| vec![None; paragraphs.len()])
                .collect()],
            cell_tabs: vec![nested_cells
                .iter()
                .map(|paragraphs| vec![Vec::new(); paragraphs.len()])
                .collect()],
        })]]];
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();
        layout_table_with_row_pagination(
            &table,
            &mut flow,
            geom,
            &mut tcx,
            &mut capture,
            TablePaginationView {
                cells: Some(&cell_pagination),
                nested: Some(&nested_pagination),
                ..TablePaginationView::default()
            },
        );
        let FlowItem::Table { mut rows, .. } = flow.remove(0) else {
            panic!("table flow item")
        };
        rows.remove(0)
    }

    fn deeply_nested_cell_row(depth: u32) -> super::RowLayout {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut cell = Cell {
            blocks: vec![Block::Paragraph(Paragraph {
                runs: vec![Run {
                    text: "bounded".to_string(),
                    ..Run::default()
                }],
                ..Paragraph::default()
            })],
            ..Cell::default()
        };
        let mut direct_hints = vec![Some(PaginationHint {
            keep_lines: true,
            ..PaginationHint::default()
        })];
        let mut nested_hints = vec![None];
        for _ in 0..depth {
            let table_hints = TablePaginationHints {
                rows: vec![TableRowPaginationHint::default()],
                cells: vec![vec![direct_hints]],
                nested: vec![vec![nested_hints]],
                cell_tabs: vec![vec![vec![Vec::new()]]],
            };
            cell = Cell {
                blocks: vec![Block::Table(Table {
                    rows: vec![Row { cells: vec![cell] }],
                    ..Table::default()
                })],
                ..Cell::default()
            };
            direct_hints = vec![None];
            nested_hints = vec![Some(table_hints)];
        }
        let table = Table {
            rows: vec![Row { cells: vec![cell] }],
            ..Table::default()
        };
        let cell_pagination = vec![vec![direct_hints]];
        let nested_pagination = vec![vec![nested_hints]];
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();
        layout_table_with_row_pagination(
            &table,
            &mut flow,
            geom,
            &mut tcx,
            &mut capture,
            TablePaginationView {
                cells: Some(&cell_pagination),
                nested: Some(&nested_pagination),
                ..TablePaginationView::default()
            },
        );
        let FlowItem::Table { mut rows, .. } = flow.remove(0) else {
            panic!("table flow item")
        };
        rows.remove(0)
    }

    fn row_avail_for_lines(row: &super::RowLayout, count: usize) -> f32 {
        let cell = &row.cells[0];
        cell.insets.top
            + cell.insets.bottom
            + cell
                .lines
                .iter()
                .take(count)
                .map(LineLayout::cell_extent)
                .sum::<f32>()
    }

    #[test]
    fn table_cell_widow_control_avoids_a_three_plus_one_split() {
        let row = cell_row_with_pagination(&[(
            "one\ntwo\nthree\nfour",
            PaginationHint {
                widow_control: true,
                ..PaginationHint::default()
            },
        )]);
        assert_eq!(row.cells[0].lines.len(), 4);
        let avail = row_avail_for_lines(&row, 3);

        let (head, tail) = split_row(row, avail);
        let tail = tail.expect("two widow-protected lines remain");

        assert_eq!(head.cells[0].lines.len(), 2);
        assert_eq!(tail.cells[0].lines.len(), 2);
    }

    #[test]
    fn table_cell_keep_lines_uses_the_last_legal_paragraph_boundary() {
        let row = cell_row_with_pagination(&[
            ("lead", PaginationHint::default()),
            (
                "one\ntwo\nthree",
                PaginationHint {
                    keep_lines: true,
                    ..PaginationHint::default()
                },
            ),
        ]);
        assert_eq!(row.cells[0].lines.len(), 4);
        let avail = row_avail_for_lines(&row, 3);

        let (head, tail) = split_row(row, avail);
        let tail = tail.expect("kept paragraph remains");

        assert_eq!(head.cells[0].lines.len(), 1);
        assert_eq!(tail.cells[0].lines.len(), 3);
    }

    #[test]
    fn table_cell_keep_next_raises_the_minimum_row_fragment() {
        let row = cell_row_with_pagination(&[
            (
                "heading",
                PaginationHint {
                    keep_next: true,
                    ..PaginationHint::default()
                },
            ),
            ("body one\nbody two", PaginationHint::default()),
        ]);
        assert_eq!(row.cells[0].lines.len(), 3);
        let cell = &row.cells[0];
        let expected = cell.insets.top + cell.lines[0].height + cell.lines[1].height;

        assert!((first_row_fragment_height(&row) - expected).abs() < 0.01);
    }

    #[test]
    fn table_cell_keep_next_chains_direct_paragraphs() {
        let keep_next = PaginationHint {
            keep_next: true,
            ..PaginationHint::default()
        };
        let row = cell_row_with_pagination(&[
            ("heading one", keep_next),
            ("heading two", keep_next),
            ("body", PaginationHint::default()),
        ]);
        let cell = &row.cells[0];
        assert_eq!(cell.lines.len(), 3);
        let expected = cell.insets.top
            + cell.lines.iter().map(|line| line.height).sum::<f32>()
            + cell.insets.bottom;

        assert!((first_row_fragment_height(&row) - expected).abs() < 0.01);
    }

    #[test]
    fn nested_table_cell_keep_next_chains_same_cell_paragraphs() {
        let keep_next = PaginationHint {
            keep_next: true,
            ..PaginationHint::default()
        };
        let row = nested_cell_row_with_pagination(&[vec![
            ("heading one", keep_next),
            ("heading two", keep_next),
            ("body", PaginationHint::default()),
        ]]);
        let cell = &row.cells[0];
        assert_eq!(cell.lines.len(), 3);
        assert_eq!(
            cell.lines[0]
                .cell_paragraph
                .expect("first nested paragraph")
                .scope_id,
            cell.lines[2]
                .cell_paragraph
                .expect("last nested paragraph")
                .scope_id
        );
        let expected = cell.insets.top
            + cell.lines.iter().map(|line| line.height).sum::<f32>()
            + cell.insets.bottom;

        assert!((first_row_fragment_height(&row) - expected).abs() < 0.01);
    }

    #[test]
    fn nested_table_cell_keep_next_does_not_cross_cell_scopes() {
        let row = nested_cell_row_with_pagination(&[
            vec![(
                "heading",
                PaginationHint {
                    keep_next: true,
                    ..PaginationHint::default()
                },
            )],
            vec![("separate cell", PaginationHint::default())],
        ]);
        let cell = &row.cells[0];
        assert_eq!(cell.lines.len(), 2);
        let first = cell.lines[0]
            .cell_paragraph
            .expect("first nested cell paragraph");
        let second = cell.lines[1]
            .cell_paragraph
            .expect("second nested cell paragraph");
        assert_ne!(first.scope_id, second.scope_id);
        let expected = cell.insets.top + cell.lines[0].height;

        assert!((first_row_fragment_height(&row) - expected).abs() < 0.01);
    }

    #[test]
    fn nested_table_cell_widow_control_avoids_a_three_plus_one_split() {
        let row = nested_cell_row_with_pagination(&[vec![(
            "one\ntwo\nthree\nfour",
            PaginationHint {
                widow_control: true,
                ..PaginationHint::default()
            },
        )]]);
        assert_eq!(row.cells[0].lines.len(), 4);
        let avail = row_avail_for_lines(&row, 3);

        let (head, tail) = split_row(row, avail);
        let tail = tail.expect("two nested widow-protected lines remain");

        assert_eq!(head.cells[0].lines.len(), 2);
        assert_eq!(tail.cells[0].lines.len(), 2);
    }

    #[test]
    fn nested_table_cant_split_requires_a_whole_first_fragment_but_allows_progress() {
        let row = nested_cell_row_with_pagination_and_row_policy(
            &[
                vec![("one\ntwo", PaginationHint::default())],
                vec![("three", PaginationHint::default())],
            ],
            true,
        );
        let cell = &row.cells[0];
        assert_eq!(cell.lines.len(), 3);
        let whole_row =
            cell.insets.top + super::cell_lines_extent(&cell.lines) + cell.insets.bottom;
        assert!((first_row_fragment_height(&row) - whole_row).abs() < 0.01);
        let avail = row_avail_for_lines(&row, 2);

        let (head, tail) = split_row(row, avail);
        let tail = tail.expect("over-tall nested row content remains");

        assert_eq!(head.cells[0].lines.len(), 2);
        assert_eq!(tail.cells[0].lines.len(), 1);
    }

    #[test]
    fn over_tall_nested_kept_table_cell_still_splits_for_progress() {
        let row = nested_cell_row_with_pagination(&[vec![(
            "one\ntwo\nthree\nfour\nfive",
            PaginationHint {
                keep_lines: true,
                ..PaginationHint::default()
            },
        )]]);
        assert_eq!(row.cells[0].lines.len(), 5);
        let avail = row_avail_for_lines(&row, 2);

        let (head, tail) = split_row(row, avail);
        let tail = tail.expect("over-tall nested kept content remains");

        assert_eq!(head.cells[0].lines.len(), 2);
        assert_eq!(tail.cells[0].lines.len(), 3);
    }

    #[test]
    fn nested_table_pagination_stops_at_the_render_depth_limit() {
        let at_limit = deeply_nested_cell_row(super::MAX_CELL_DEPTH);
        let beyond_limit = deeply_nested_cell_row(super::MAX_CELL_DEPTH + 1);

        assert_eq!(at_limit.cells[0].lines.len(), 1);
        assert!(beyond_limit.cells[0].lines.is_empty());
    }

    #[test]
    fn table_cell_widow_control_keeps_a_short_paragraph_whole() {
        let row = cell_row_with_pagination(&[(
            "one\ntwo\nthree",
            PaginationHint {
                widow_control: true,
                ..PaginationHint::default()
            },
        )]);
        let cell = &row.cells[0];
        assert_eq!(cell.lines.len(), 3);
        let expected = cell.insets.top
            + cell.lines.iter().map(|line| line.height).sum::<f32>()
            + cell.insets.bottom;

        assert!((first_row_fragment_height(&row) - expected).abs() < 0.01);
    }

    #[test]
    fn table_cells_choose_independent_legal_split_points() {
        let mut protected = cell_row_with_pagination(&[(
            "one\ntwo\nthree\nfour",
            PaginationHint {
                widow_control: true,
                ..PaginationHint::default()
            },
        )]);
        let mut plain =
            cell_row_with_pagination(&[("alpha\nbeta\ngamma\ndelta", PaginationHint::default())]);
        let avail = row_avail_for_lines(&protected, 3);
        let row = super::RowLayout {
            height: protected.height.max(plain.height),
            cells: vec![protected.cells.remove(0), plain.cells.remove(0)],
            cant_split: false,
            border: super::TableBorderPaints::default(),
            table_id: None,
        };

        let (head, tail) = split_row(row, avail);
        let tail = tail.expect("both cells have remaining lines");

        assert_eq!(head.cells[0].lines.len(), 2);
        assert_eq!(head.cells[1].lines.len(), 3);
        assert_eq!(tail.cells[0].lines.len(), 2);
        assert_eq!(tail.cells[1].lines.len(), 1);
    }

    #[test]
    fn multi_fragment_table_row_preserves_only_true_outer_horizontal_edges() {
        let row = pagination_table_row(false, 3);
        let (first, rest) = split_row(row, 10.0);
        let (middle, last) = split_row(rest.expect("two lines remain"), 10.0);
        let last = last.expect("one line remains");

        assert_eq!(first.cells[0].border_edges.top, Some(TableBorderSide::Top));
        assert_eq!(first.cells[0].border_edges.bottom, None);
        assert_eq!(middle.cells[0].border_edges.top, None);
        assert_eq!(middle.cells[0].border_edges.bottom, None);
        assert_eq!(last.cells[0].border_edges.top, None);
        assert_eq!(
            last.cells[0].border_edges.bottom,
            Some(TableBorderSide::Bottom)
        );
    }

    #[test]
    fn over_tall_kept_table_cell_still_splits_for_progress() {
        let row = cell_row_with_pagination(&[(
            "one\ntwo\nthree\nfour\nfive",
            PaginationHint {
                keep_lines: true,
                ..PaginationHint::default()
            },
        )]);
        assert_eq!(row.cells[0].lines.len(), 5);
        let avail = row_avail_for_lines(&row, 2);

        let (head, tail) = split_row(row, avail);
        let tail = tail.expect("over-tall kept content remains");

        assert_eq!(head.cells[0].lines.len(), 2);
        assert_eq!(tail.cells[0].lines.len(), 3);
    }

    #[test]
    fn strict_registered_font_shapes_latin_and_korean() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };

        let lines = shape(
            "Latin 한글 paragraph",
            StyledText::plain(&[(0, "Latin 한글 paragraph".len(), CharProps::default())]),
            None,
            parley::layout::Alignment::Start,
            320.0,
            &mut tcx,
        );

        assert!(
            !lines.is_empty(),
            "strict registered font produced no lines"
        );
        assert!(
            lines.iter().map(|line| line.height).sum::<f32>() > 0.0,
            "strict registered font produced zero layout height"
        );
    }

    #[test]
    fn strict_bundled_fonts_shape_arabic_and_hebrew_without_notdef_glyphs() {
        let fonts = vec![
            rwml_fonts::noto_sans_arabic_subset().to_vec(),
            rwml_fonts::noto_sans_hebrew_subset().to_vec(),
        ];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let arabic = "سلام ١٢٣";
        let hebrew = "שלום 12,34";
        let arabic_lines = shape(
            arabic,
            StyledText::plain(&[(0, arabic.len(), CharProps::default())]),
            None,
            parley::layout::Alignment::Start,
            320.0,
            &mut tcx,
        );
        let hebrew_lines = shape(
            hebrew,
            StyledText::plain(&[(0, hebrew.len(), CharProps::default())]),
            None,
            parley::layout::Alignment::Start,
            320.0,
            &mut tcx,
        );
        let isolated_arabic = "س ل ا م";
        let isolated_arabic_lines = shape(
            isolated_arabic,
            StyledText::plain(&[(0, isolated_arabic.len(), CharProps::default())]),
            None,
            parley::layout::Alignment::Start,
            320.0,
            &mut tcx,
        );

        for lines in [&arabic_lines, &hebrew_lines] {
            assert!(!lines.is_empty());
            assert!(lines
                .iter()
                .flat_map(|line| &line.runs)
                .flat_map(|run| &run.glyphs)
                .all(|glyph| glyph.glyph_id.to_u32() != 0));
        }
        let mut joined_ids = arabic_lines
            .iter()
            .flat_map(|line| &line.runs)
            .flat_map(|run| &run.glyphs)
            .filter(|glyph| {
                arabic[glyph.text_range.clone()]
                    .chars()
                    .any(|ch| matches!(ch, 'س' | 'ل' | 'ا' | 'م'))
            })
            .map(|glyph| glyph.glyph_id.to_u32())
            .collect::<Vec<_>>();
        let mut isolated_ids = isolated_arabic_lines
            .iter()
            .flat_map(|line| &line.runs)
            .flat_map(|run| &run.glyphs)
            .filter(|glyph| {
                isolated_arabic[glyph.text_range.clone()]
                    .chars()
                    .any(|ch| matches!(ch, 'س' | 'ل' | 'ا' | 'م'))
            })
            .map(|glyph| glyph.glyph_id.to_u32())
            .collect::<Vec<_>>();
        joined_ids.sort_unstable();
        isolated_ids.sort_unstable();
        assert_ne!(joined_ids, isolated_ids);
    }

    #[test]
    fn strict_garbage_font_bytes_do_not_panic() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut font_cx = strict_font_context(&[vec![1, 2, 3, 4, 5]]);
            let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
            let mut font_cache = HashMap::new();
            let mut tcx = TextCx {
                font_cx: &mut font_cx,
                layout_cx: &mut layout_cx,
                font_cache: &mut font_cache,
            };
            let _ = shape(
                "Latin 한글 paragraph",
                StyledText::plain(&[(0, "Latin 한글 paragraph".len(), CharProps::default())]),
                None,
                parley::layout::Alignment::Start,
                320.0,
                &mut tcx,
            );
        }));

        assert!(result.is_ok(), "garbage strict font bytes panicked");
    }

    #[test]
    fn color_and_link_lookup_are_correct_after_binary_search() {
        use super::{color_at, link_at, rgb};
        use crate::model::{CharProps, Color};
        use std::rc::Rc;
        let red = CharProps {
            color: Some(Color { r: 255, g: 0, b: 0 }),
            ..Default::default()
        };
        // [0,5)=red, gap [5,10), [10,15)=default (no color) — ordered, non-overlapping.
        let ranges = vec![
            (0usize, 5usize, red),
            (10usize, 15usize, CharProps::default()),
        ];
        assert_eq!(color_at(&ranges, 0), rgb::Color::new(255, 0, 0));
        assert_eq!(color_at(&ranges, 4), rgb::Color::new(255, 0, 0));
        assert_eq!(color_at(&ranges, 5), rgb::Color::new(0, 0, 0)); // gap → black
        assert_eq!(color_at(&ranges, 12), rgb::Color::new(0, 0, 0)); // range w/o color
        assert_eq!(color_at(&ranges, 99), rgb::Color::new(0, 0, 0)); // past end
        assert_eq!(color_at(&[], 0), rgb::Color::new(0, 0, 0)); // empty

        let u: Rc<str> = Rc::from("http://x");
        let links = vec![(2usize, 6usize, u)];
        assert_eq!(link_at(&links, 1), None); // before
        assert_eq!(link_at(&links, 2).as_deref(), Some("http://x"));
        assert_eq!(link_at(&links, 5).as_deref(), Some("http://x"));
        assert_eq!(link_at(&links, 6), None); // end exclusive
        assert_eq!(link_at(&[], 0), None);
    }

    #[test]
    fn maps_symbol_font_text_to_unicode_for_rendering() {
        let symbol = CharProps {
            font: Some("Symbol".to_string()),
            ..CharProps::default()
        };
        assert_eq!(display_text(&symbol, "abg\u{00D3}"), "αβγ©");

        let wingdings = CharProps {
            font: Some("Wingdings".to_string()),
            ..CharProps::default()
        };
        assert_eq!(display_text(&wingdings, "AJ\u{00FC}"), "✌☺✓");
    }

    #[test]
    fn page_field_text_uses_current_page_when_available() {
        let field = FieldRole::Simple {
            instruction: "PAGE".to_string(),
        };
        assert_eq!(
            page_field_text(&CharProps::default(), "1", &field, Some(7)),
            "7"
        );
        assert_eq!(
            page_field_text(&CharProps::default(), "1", &field, None),
            "1"
        );

        let filename = FieldRole::Simple {
            instruction: "FILENAME \\p".to_string(),
        };
        assert_eq!(
            page_field_text(&CharProps::default(), "report.docx", &filename, Some(7)),
            "report.docx"
        );
    }

    #[test]
    fn unsupported_placeholder_texts_cover_preserved_objects_only() {
        let features = FeatureInventory {
            fields: 3,
            floating_shapes: 2,
            charts: 1,
            ole_objects: 4,
            unsupported_metafiles: 5,
            ..FeatureInventory::default()
        };

        assert_eq!(
            unsupported_placeholder_texts(&features),
            vec![
                "[rwml preview placeholder: 2 floating shapes preserved but not positioned]",
                "[rwml preview placeholder: 1 chart preserved but not modeled]",
                "[rwml preview placeholder: 4 OLE objects preserved but not modeled]",
                "[rwml preview placeholder: 5 WMF/EMF images preserved but not rendered]",
            ]
        );
    }

    #[test]
    fn known_floating_shape_overlays_reduce_aggregate_placeholder_count() {
        let features = FeatureInventory {
            floating_shapes: 3,
            charts: 1,
            ..FeatureInventory::default()
        };

        assert_eq!(
            super::unsupported_placeholder_texts_with_known_shapes(&features, 2),
            vec![
                "[rwml preview placeholder: 1 floating shape preserved but not positioned]",
                "[rwml preview placeholder: 1 chart preserved but not modeled]",
            ]
        );
        assert_eq!(
            super::unsupported_placeholder_texts_with_known_shapes(&features, 3),
            vec!["[rwml preview placeholder: 1 chart preserved but not modeled]"]
        );
    }

    #[test]
    fn undecodable_image_placeholder_texts_describe_skipped_rasters() {
        assert!(super::undecodable_image_placeholder_texts(0).is_empty());
        assert_eq!(
            super::undecodable_image_placeholder_texts(2),
            vec![
                "[rwml preview placeholder: 2 raster images skipped because the PDF backend could not decode them]"
            ]
        );
    }

    #[test]
    fn missing_image_placeholder_texts_describe_unavailable_bytes() {
        assert!(super::missing_image_placeholder_texts(0).is_empty());
        assert_eq!(
            super::missing_image_placeholder_texts(2),
            vec![
                "[rwml preview placeholder: 2 images unavailable because their bytes were not extracted]"
            ]
        );
    }

    #[test]
    fn floating_shape_overlays_use_anchor_geometry() {
        let geom = Geom::from_setup(&PageSetup::default());
        let overlays = super::floating_shape_overlays_for_pages(
            &[FloatingShape {
                id: "7".to_string(),
                name: Some("Float one".to_string()),
                description: Some("A floating object".to_string()),
                text: Some("Shape body".to_string()),
                preset_geometry: Some("roundRect".to_string()),
                fill_color: Some(Color::rgb(0xFF, 0x88, 0x00)),
                outline_color: Some(Color::rgb(0x00, 0x33, 0x66)),
                simple_position_enabled: Some(true),
                simple_position: Some(ShapePoint {
                    x_emu: 182_880,
                    y_emu: 274_320,
                }),
                effect_extent: Some(ShapeEffectExtent {
                    left_emu: 9_144,
                    top_emu: 18_288,
                    right_emu: 27_432,
                    bottom_emu: 36_576,
                }),
                anchor_block_index: Some(0),
                anchor_text: Some("Before anchor After anchor".to_string()),
                anchor_char_offset: Some("Before anchor ".chars().count()),
                extent: Some(ShapeExtent {
                    cx_emu: 914_400,
                    cy_emu: 457_200,
                }),
                horizontal_position: Some(ShapePosition {
                    relative_from: Some("column".to_string()),
                    offset_emu: Some(91_440),
                    align: None,
                }),
                vertical_position: Some(ShapePosition {
                    relative_from: Some("paragraph".to_string()),
                    offset_emu: None,
                    align: Some("top".to_string()),
                }),
                relative_height: Some(251_659_264),
                behind_doc: Some(false),
                layout_in_cell: Some(true),
                locked: Some(false),
                allow_overlap: Some(true),
                distance: crate::ShapeDistance {
                    top_emu: Some(12_700),
                    bottom_emu: Some(25_400),
                    left_emu: Some(38_100),
                    right_emu: Some(50_800),
                },
                wrapping: Some(crate::ShapeWrapping {
                    kind: "square".to_string(),
                    text: Some("bothSides".to_string()),
                    distance: crate::ShapeDistance {
                        top_emu: Some(9_144),
                        bottom_emu: Some(18_288),
                        left_emu: Some(27_432),
                        right_emu: Some(36_576),
                    },
                    polygon: vec![
                        ShapePoint { x_emu: 0, y_emu: 0 },
                        ShapePoint {
                            x_emu: 914_400,
                            y_emu: 0,
                        },
                        ShapePoint {
                            x_emu: 914_400,
                            y_emu: 457_200,
                        },
                        ShapePoint {
                            x_emu: 0,
                            y_emu: 457_200,
                        },
                    ],
                }),
            }],
            geom,
            &HashMap::new(),
            &HashMap::new(),
        );

        assert_eq!(overlays.len(), 1);
        let overlay = &overlays[0];
        assert!((overlay.x - 14.4).abs() < 0.01);
        assert!((overlay.y - 21.6).abs() < 0.01);
        assert!((overlay.w - 72.0).abs() < 0.01);
        assert!((overlay.h - 36.0).abs() < 0.01);
        assert_eq!(overlay.page_index, 0);
        assert_eq!(
            overlay.label,
            "floating shape 1: Float one (72 x 36 pt, x simplePos 14.4 pt, y simplePos 21.6 pt, z 251659264, front, anchor dist t 1 pt, b 2 pt, l 3 pt, r 4 pt, wrap square bothSides wrap dist t 0.7 pt, b 1.4 pt, l 2.2 pt, r 2.9 pt wrap polygon 4 pts, geometry roundRect, effect l 0.7 pt, t 1.4 pt, r 2.2 pt, b 2.9 pt, fill #FF8800, outline #003366, anchor Before anchor After anchor, text Shape body)"
        );
    }

    #[test]
    fn floating_shape_coordinates_use_distinct_physical_margin_bands() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 600.0,
            height_pt: 800.0,
            margin_pt: 72.0,
            margin_left_pt: Some(60.0),
            margin_right_pt: Some(90.0),
            margin_top_pt: Some(72.0),
            margin_bottom_pt: Some(108.0),
            landscape: false,
        });
        let coordinate = |axis, relative_from: &str, align: Option<&str>, size| {
            super::floating_shape_coordinate(
                Some(&ShapePosition {
                    relative_from: Some(relative_from.to_string()),
                    offset_emu: align.is_none().then_some(0),
                    align: align.map(str::to_string),
                }),
                axis,
                geom,
                size,
            )
        };

        let horizontal = [
            ("leftMargin", None, 0.0),
            ("leftMargin", Some("center"), 15.0),
            ("rightMargin", None, 510.0),
            ("rightMargin", Some("center"), 540.0),
            ("margin", None, 60.0),
            ("page", None, 0.0),
        ];
        for (relative_from, align, expected) in horizontal {
            let actual = coordinate(super::ShapeAxis::Horizontal, relative_from, align, 30.0);
            assert!(
                (actual - expected).abs() < 0.01,
                "horizontal {relative_from:?} {align:?}: expected {expected}, got {actual}"
            );
        }

        let vertical = [
            ("topMargin", None, 0.0),
            ("topMargin", Some("center"), 16.0),
            ("bottomMargin", None, 692.0),
            ("bottomMargin", Some("center"), 726.0),
            ("margin", None, 72.0),
            ("page", None, 0.0),
        ];
        for (relative_from, align, expected) in vertical {
            let actual = coordinate(super::ShapeAxis::Vertical, relative_from, align, 40.0);
            assert!(
                (actual - expected).abs() < 0.01,
                "vertical {relative_from:?} {align:?}: expected {expected}, got {actual}"
            );
        }
    }

    #[test]
    fn floating_shape_overlays_follow_anchor_z_order() {
        let geom = Geom::from_setup(&PageSetup::default());
        let overlays = super::floating_shape_overlays_for_pages(
            &[
                FloatingShape {
                    id: "front".to_string(),
                    name: Some("Front".to_string()),
                    description: None,
                    text: None,
                    preset_geometry: None,
                    fill_color: None,
                    outline_color: None,
                    simple_position_enabled: None,
                    simple_position: None,
                    effect_extent: None,
                    anchor_block_index: None,
                    anchor_text: None,
                    anchor_char_offset: None,
                    extent: None,
                    horizontal_position: None,
                    vertical_position: None,
                    relative_height: Some(20),
                    behind_doc: Some(false),
                    layout_in_cell: None,
                    locked: None,
                    allow_overlap: None,
                    distance: crate::ShapeDistance::default(),
                    wrapping: None,
                },
                FloatingShape {
                    id: "back".to_string(),
                    name: Some("Back".to_string()),
                    description: None,
                    text: None,
                    preset_geometry: None,
                    fill_color: None,
                    outline_color: None,
                    simple_position_enabled: None,
                    simple_position: None,
                    effect_extent: None,
                    anchor_block_index: None,
                    anchor_text: None,
                    anchor_char_offset: None,
                    extent: None,
                    horizontal_position: None,
                    vertical_position: None,
                    relative_height: Some(10),
                    behind_doc: Some(true),
                    layout_in_cell: None,
                    locked: None,
                    allow_overlap: None,
                    distance: crate::ShapeDistance::default(),
                    wrapping: None,
                },
            ],
            geom,
            &HashMap::new(),
            &HashMap::new(),
        );

        assert_eq!(overlays.len(), 2);
        assert!(overlays[0].behind_doc);
        assert!(!overlays[1].behind_doc);
        assert!(overlays[0].label.contains("Back"));
        assert!(overlays[1].label.contains("Front"));
    }

    #[test]
    fn floating_shape_overlays_use_anchor_block_page() {
        let geom = Geom::from_setup(&PageSetup::default());
        let mut block_pages = HashMap::new();
        block_pages.insert(2, 1);
        let overlays = super::floating_shape_overlays_for_pages(
            &[FloatingShape {
                id: "late".to_string(),
                name: Some("Late".to_string()),
                description: None,
                text: None,
                preset_geometry: None,
                fill_color: None,
                outline_color: None,
                simple_position_enabled: None,
                simple_position: None,
                effect_extent: None,
                anchor_block_index: Some(2),
                anchor_text: None,
                anchor_char_offset: None,
                extent: None,
                horizontal_position: None,
                vertical_position: None,
                relative_height: None,
                behind_doc: None,
                layout_in_cell: None,
                locked: None,
                allow_overlap: None,
                distance: crate::ShapeDistance::default(),
                wrapping: None,
            }],
            geom,
            &block_pages,
            &HashMap::new(),
        );

        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].page_index, 1);
    }

    #[test]
    fn floating_shape_overlays_use_anchor_line_page_for_spanning_block() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 90.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            margin_left_pt: None,
            margin_right_pt: None,
            margin_top_pt: None,
            margin_bottom_pt: None,
            landscape: false,
        });
        let mut font_cx = FontContext::default();
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi \
            omicron pi rho sigma tau upsilon phi chi psi omega alpha beta gamma delta epsilon \
            zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi \
            psi omega anchor target";
        let lines = shape(
            text,
            StyledText::plain(&[(0, text.len(), CharProps::default())]),
            None,
            parley::layout::Alignment::Start,
            geom.content_w(),
            &mut tcx,
        );
        let mut items = vec![FlowItem::BlockStart {
            index: 0,
            pagination: super::PaginationHint::default(),
        }];
        items.extend(lines.clone().into_iter().map(FlowItem::Line));
        let pagination = paginate(items, geom, &SectionSetup::default());
        assert!(
            pagination.pages.len() >= 2,
            "fixture paragraph should span at least two pages"
        );
        let page_two_anchor_offset = pagination.pages[1]
            .iter()
            .find_map(|placed| match &placed.item {
                FlowItem::Line(line) => {
                    line_char_range(line, text).map(|(start, end)| start.saturating_add(1).min(end))
                }
                _ => None,
            })
            .expect("page-two line range");

        let overlays = super::floating_shape_overlays_for_pages(
            &[FloatingShape {
                id: "late-anchor".to_string(),
                name: Some("Late anchor".to_string()),
                description: None,
                text: None,
                preset_geometry: None,
                fill_color: None,
                outline_color: None,
                simple_position_enabled: None,
                simple_position: None,
                effect_extent: None,
                anchor_block_index: Some(0),
                anchor_text: Some(text.to_string()),
                anchor_char_offset: Some(page_two_anchor_offset),
                extent: None,
                horizontal_position: None,
                vertical_position: None,
                relative_height: None,
                behind_doc: None,
                layout_in_cell: None,
                locked: None,
                allow_overlap: None,
                distance: crate::ShapeDistance::default(),
                wrapping: None,
            }],
            geom,
            &pagination.block_pages,
            &pagination.block_line_pages,
        );

        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].page_index, 1);
    }

    #[test]
    fn lays_out_dynamic_page_number_footer_line() {
        let geom = Geom::from_setup(&PageSetup::default());
        let mut font_cx = FontContext::default();
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };

        let line = layout_page_number_line(7, geom, &mut tcx).expect("page number line");
        let text: String = line.runs.iter().map(|run| run.text.as_ref()).collect();
        assert_eq!(text, "7");
        assert!(
            line.runs.iter().any(|run| run.x > geom.content_w() * 0.4),
            "page number should be centered in the content box"
        );
    }

    #[test]
    fn selects_first_even_and_default_running_surfaces_by_page() {
        let setup = crate::model::DocSetup {
            header: vec![para("default header", None)],
            first_header: vec![para("first header", None)],
            even_header: vec![para("even header", None)],
            footer: vec![para("default footer", None)],
            first_footer: vec![para("first footer", None)],
            even_footer: vec![para("even footer", None)],
            ..Default::default()
        };

        let (header, footer) = running_header_footer_blocks_for_page(&setup, 1, true);
        assert_eq!(block_text(header), "first header");
        assert_eq!(block_text(footer), "first footer");

        let (header, footer) = running_header_footer_blocks_for_page(&setup, 2, false);
        assert_eq!(block_text(header), "even header");
        assert_eq!(block_text(footer), "even footer");

        let (header, footer) = running_header_footer_blocks_for_page(&setup, 3, false);
        assert_eq!(block_text(header), "default header");
        assert_eq!(block_text(footer), "default footer");
    }

    #[test]
    fn running_surface_selection_falls_back_to_default_when_variant_is_empty() {
        let setup = crate::model::DocSetup {
            header: vec![para("default header", None)],
            footer: vec![para("default footer", None)],
            even_header: vec![para("even header", None)],
            ..Default::default()
        };

        let (header, footer) = running_header_footer_blocks_for_page(&setup, 1, true);
        assert_eq!(block_text(header), "default header");
        assert_eq!(block_text(footer), "default footer");

        let (header, footer) = running_header_footer_blocks_for_page(&setup, 2, false);
        assert_eq!(block_text(header), "even header");
        assert_eq!(block_text(footer), "default footer");
    }

    #[test]
    fn title_page_suppresses_default_running_surface_on_first_page() {
        let setup = crate::model::DocSetup {
            header: vec![para("default header", None)],
            footer: vec![para("default footer", None)],
            title_page: true,
            ..Default::default()
        };

        let (header, footer) = running_header_footer_blocks_for_page(&setup, 1, true);
        assert!(header.is_empty());
        assert!(footer.is_empty());

        let (header, footer) = running_header_footer_blocks_for_page(&setup, 2, false);
        assert_eq!(block_text(header), "default header");
        assert_eq!(block_text(footer), "default footer");
    }

    #[test]
    fn selects_first_running_surface_on_first_page_of_later_section() {
        let setup = SectionSetup {
            header: vec![para("section default header", None)],
            first_header: vec![para("section first header", None)],
            even_header: vec![para("section even header", None)],
            footer: vec![para("section default footer", None)],
            first_footer: vec![para("section first footer", None)],
            even_footer: vec![para("section even footer", None)],
            ..Default::default()
        };

        let (header, footer) = running_header_footer_blocks_for_page(&setup, 3, true);
        assert_eq!(block_text(header), "section first header");
        assert_eq!(block_text(footer), "section first footer");

        let (header, footer) = running_header_footer_blocks_for_page(&setup, 4, false);
        assert_eq!(block_text(header), "section even header");
        assert_eq!(block_text(footer), "section even footer");
    }

    #[test]
    fn section_page_assignment_tracks_section_start_page() {
        let first = SectionSetup {
            header: vec![para("first default header", None)],
            first_header: vec![para("first first header", None)],
            even_header: vec![para("first even header", None)],
            ..Default::default()
        };
        let final_setup = SectionSetup {
            header: vec![para("final default header", None)],
            first_header: vec![para("final first header", None)],
            even_header: vec![para("final even header", None)],
            ..Default::default()
        };
        let mut page_sections = vec![None, None, None, None];

        assign_section_to_render_pages(&mut page_sections, 0, 1, &first);
        assign_section_to_render_pages(&mut page_sections, 2, 3, &final_setup);

        let first_page = page_sections[0].as_ref().expect("first page section");
        assert_eq!(first_page.first_page_index, 0);
        let second_page = page_sections[1].as_ref().expect("second page section");
        assert_eq!(second_page.first_page_index, 0);
        let final_first_page = page_sections[2].as_ref().expect("final first page section");
        assert_eq!(final_first_page.first_page_index, 2);

        let (header, _) = running_header_footer_blocks_for_page(
            &final_first_page.setup,
            3,
            final_first_page.first_page_index == 2,
        );
        assert_eq!(block_text(header), "final first header");

        let final_second_page = page_sections[3]
            .as_ref()
            .expect("final second page section");
        let (header, _) = running_header_footer_blocks_for_page(
            &final_second_page.setup,
            4,
            final_second_page.first_page_index == 3,
        );
        assert_eq!(block_text(header), "final even header");
    }

    #[test]
    fn pagination_selects_later_section_running_surface_after_section_break() {
        let first = SectionSetup {
            header: vec![para("first section header", None)],
            ..Default::default()
        };
        let final_setup = SectionSetup {
            header: vec![para("second default header", None)],
            first_header: vec![para("second first header", None)],
            ..Default::default()
        };
        let pagination = paginate(
            vec![FlowItem::SectionBreak(first)],
            Geom::from_setup(&PageSetup::default()),
            &final_setup,
        );

        let second_page = pagination.page_sections[1]
            .as_ref()
            .expect("second page section");
        let (header, _) = running_header_footer_blocks_for_page(
            &second_page.setup,
            2,
            second_page.first_page_index == 1,
        );
        assert_eq!(block_text(header), "second first header");
    }

    #[test]
    fn parity_filler_stays_in_ending_section_and_target_stays_first_page() {
        let ending = SectionSetup {
            section_break: Some(SectionBreakKind::OddPage),
            header: vec![para("ending section", None)],
            ..SectionSetup::default()
        };
        let final_setup = SectionSetup {
            header: vec![para("following section", None)],
            ..SectionSetup::default()
        };
        let pagination = paginate(
            vec![
                pagination_line(10.0),
                FlowItem::SectionBreak(ending),
                pagination_line(10.0),
            ],
            Geom::from_setup(&PageSetup::default()),
            &final_setup,
        );

        assert_eq!(page_line_counts(&pagination), vec![1, 0, 1]);
        let filler = pagination.page_sections[1]
            .as_ref()
            .expect("parity filler section");
        assert_eq!(block_text(&filler.setup.header), "ending section");
        assert_eq!(filler.first_page_index, 0);
        let target = pagination.page_sections[2]
            .as_ref()
            .expect("following section target");
        assert_eq!(block_text(&target.setup.header), "following section");
        assert_eq!(target.first_page_index, 2);
    }

    #[test]
    fn parity_uses_pages_created_by_automatic_overflow() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let ending = SectionSetup {
            section_break: Some(SectionBreakKind::EvenPage),
            header: vec![para("ending section", None)],
            ..SectionSetup::default()
        };
        let final_setup = SectionSetup {
            header: vec![para("following section", None)],
            ..SectionSetup::default()
        };
        let mut items = (0..5).map(|_| pagination_line(20.0)).collect::<Vec<_>>();
        items.push(FlowItem::SectionBreak(ending));
        items.push(pagination_line(20.0));

        let pagination = paginate(items, geom, &final_setup);

        assert_eq!(page_line_counts(&pagination), vec![3, 2, 0, 1]);
        assert_eq!(
            block_text(
                &pagination.page_sections[2]
                    .as_ref()
                    .expect("parity filler section")
                    .setup
                    .header
            ),
            "ending section"
        );
        let target = pagination.page_sections[3]
            .as_ref()
            .expect("following section target");
        assert_eq!(block_text(&target.setup.header), "following section");
        assert_eq!(target.first_page_index, 3);
    }

    #[test]
    fn equal_width_columns_fill_across_before_creating_a_page() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let line = || {
            FlowItem::Line(LineLayout {
                height: 10.0,
                baseline: 8.0,
                x_indent: 0.0,
                char_range: None,
                background: None,
                cell_spacing: Default::default(),
                cell_paragraph: None,
                cell_cant_split_group: None,
                runs: Vec::new(),
            })
        };
        let setup = SectionSetup {
            columns: Some(2),
            ..SectionSetup::default()
        };

        let pagination = paginate((0..8).map(|_| line()).collect(), geom, &setup);

        assert_eq!(pagination.pages.len(), 1);
        let x_positions = pagination.pages[0]
            .iter()
            .filter_map(|placed| matches!(&placed.item, FlowItem::Line(_)).then_some(placed.x))
            .collect::<Vec<_>>();
        assert_eq!(x_positions.len(), 8);
        assert!(x_positions[..6].iter().all(|x| x.abs() < 0.1));
        assert!(x_positions[6..].iter().all(|x| *x > 90.0));
    }

    fn pagination_line(height: f32) -> FlowItem {
        FlowItem::Line(LineLayout {
            height,
            baseline: height * 0.8,
            x_indent: 0.0,
            char_range: None,
            background: None,
            cell_spacing: Default::default(),
            cell_paragraph: None,
            cell_cant_split_group: None,
            runs: Vec::new(),
        })
    }

    fn pagination_line_with_range(height: f32, start: usize, end: usize) -> FlowItem {
        let FlowItem::Line(mut line) = pagination_line(height) else {
            unreachable!()
        };
        line.char_range = Some(super::LineCharRange { start, end });
        FlowItem::Line(line)
    }

    fn pagination_block(index: usize, pagination: PaginationHint) -> FlowItem {
        FlowItem::BlockStart { index, pagination }
    }

    fn pagination_table_row(cant_split: bool, line_count: usize) -> super::RowLayout {
        let lines = (0..line_count)
            .map(|_| LineLayout {
                height: 10.0,
                baseline: 8.0,
                x_indent: 0.0,
                char_range: None,
                background: None,
                cell_spacing: Default::default(),
                cell_paragraph: None,
                cell_cant_split_group: None,
                runs: Vec::new(),
            })
            .collect::<Vec<_>>();
        super::RowLayout {
            height: line_count as f32 * 10.0,
            cells: vec![super::CellBox {
                x: 0.0,
                right: 100.0,
                width: 100.0,
                lines,
                insets: super::CellInsets::zero(),
                shading: None,
                valign: crate::model::VCell::Top,
                border_edges: super::CellBorderEdges::outer(),
            }],
            cant_split,
            border: super::TableBorderPaints::default(),
            table_id: None,
        }
    }

    fn page_row_counts(pagination: &super::Pagination) -> Vec<usize> {
        pagination
            .pages
            .iter()
            .map(|page| {
                page.iter()
                    .filter(|placed| matches!(placed.item, FlowItem::Row(_)))
                    .count()
            })
            .collect()
    }

    #[test]
    fn table_row_break_policy_uses_remaining_space_or_moves_whole() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let splittable = paginate(
            vec![
                pagination_line(30.0),
                FlowItem::Table {
                    rows: vec![pagination_table_row(false, 4)],
                    header_rows: 0,
                },
            ],
            geom,
            &SectionSetup::default(),
        );
        let kept = paginate(
            vec![
                pagination_line(30.0),
                FlowItem::Table {
                    rows: vec![pagination_table_row(true, 4)],
                    header_rows: 0,
                },
            ],
            geom,
            &SectionSetup::default(),
        );

        assert_eq!(page_row_counts(&splittable), vec![1, 1]);
        assert_eq!(page_row_counts(&kept), vec![0, 1]);
    }

    #[test]
    fn table_block_page_tracks_the_first_placed_row() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let pagination = paginate(
            vec![
                pagination_block(0, PaginationHint::default()),
                pagination_line(45.0),
                pagination_block(1, PaginationHint::default()),
                FlowItem::Table {
                    rows: vec![pagination_table_row(true, 2)],
                    header_rows: 0,
                },
            ],
            geom,
            &SectionSetup::default(),
        );

        assert_eq!(page_row_counts(&pagination), vec![0, 1]);
        assert_eq!(pagination.block_pages.get(&1), Some(&1));
    }

    #[test]
    fn table_cell_spacing_alone_can_move_the_first_row_and_its_block_page() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let compact_row = pagination_table_row(true, 1);
        let mut spaced_row = compact_row.clone();
        spaced_row.cells[0].lines[0].cell_spacing = super::CellLineSpacing {
            before: 8.0,
            after: 6.0,
        };
        spaced_row.height += 14.0;
        let paginate_row = |row| {
            paginate(
                vec![
                    pagination_block(0, PaginationHint::default()),
                    pagination_line(45.0),
                    pagination_block(1, PaginationHint::default()),
                    FlowItem::Table {
                        rows: vec![row],
                        header_rows: 0,
                    },
                ],
                geom,
                &SectionSetup::default(),
            )
        };

        let compact = paginate_row(compact_row);
        let spaced = paginate_row(spaced_row);

        assert_eq!(page_row_counts(&compact), vec![1]);
        assert_eq!(compact.block_pages.get(&1), Some(&0));
        assert_eq!(page_row_counts(&spaced), vec![0, 1]);
        assert_eq!(spaced.block_pages.get(&1), Some(&1));
    }

    #[test]
    fn spanning_table_block_page_stays_on_its_first_row_fragment() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let pagination = paginate(
            vec![
                pagination_block(0, PaginationHint::default()),
                FlowItem::Table {
                    rows: vec![
                        pagination_table_row(false, 8),
                        pagination_table_row(true, 1),
                    ],
                    header_rows: 0,
                },
            ],
            geom,
            &SectionSetup::default(),
        );

        assert!(pagination.pages.len() > 1);
        assert_eq!(pagination.block_pages.get(&0), Some(&0));
    }

    #[test]
    fn over_tall_cant_split_row_starts_fresh_and_still_makes_progress() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let pagination = paginate(
            vec![
                pagination_line(10.0),
                FlowItem::Table {
                    rows: vec![pagination_table_row(true, 8)],
                    header_rows: 0,
                },
            ],
            geom,
            &SectionSetup::default(),
        );

        assert_eq!(page_row_counts(&pagination), vec![0, 1, 1]);
    }

    #[test]
    fn splittable_row_moves_when_remainder_cannot_hold_a_line() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let pagination = paginate(
            vec![
                pagination_line(55.0),
                FlowItem::Table {
                    rows: vec![pagination_table_row(false, 2)],
                    header_rows: 0,
                },
            ],
            geom,
            &SectionSetup::default(),
        );

        assert_eq!(page_row_counts(&pagination), vec![0, 1]);
    }

    #[test]
    fn splittable_row_moves_when_remainder_cannot_hold_single_line_cell_insets() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut row = pagination_table_row(false, 1);
        row.cells[0].insets.top = 3.0;
        row.cells[0].insets.bottom = 5.0;
        row.height = 18.0;
        let pagination = paginate(
            vec![
                pagination_line(45.0),
                FlowItem::Table {
                    rows: vec![row],
                    header_rows: 0,
                },
            ],
            geom,
            &SectionSetup::default(),
        );

        assert_eq!(page_row_counts(&pagination), vec![0, 1]);
    }

    #[test]
    fn split_table_row_repeats_headers_once_per_new_page() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let header_line = super::TableBorderPaint {
            color: rgb::Color::new(0xA6, 0x1B, 0x29),
            width: 3.0,
        };
        let header_paint = super::TableBorderPaints {
            top: header_line,
            left: header_line,
            bottom: header_line,
            right: header_line,
            inside_h: header_line,
            inside_v: header_line,
        };
        let mut header = pagination_table_row(true, 1);
        header.border = header_paint;
        let pagination = paginate(
            vec![
                pagination_line(30.0),
                FlowItem::Table {
                    rows: vec![header, pagination_table_row(false, 4)],
                    header_rows: 1,
                },
            ],
            geom,
            &SectionSetup::default(),
        );

        assert_eq!(page_row_counts(&pagination), vec![2, 2]);
        let repeated_header = pagination.pages[1]
            .iter()
            .find_map(|placed| match &placed.item {
                FlowItem::Row(row) => Some(row),
                _ => None,
            })
            .expect("second page starts with repeated header");
        assert_eq!(repeated_header.border, header_paint);
        assert_eq!(
            repeated_header.cells[0].border_edges,
            super::CellBorderEdges::outer()
        );
    }

    #[test]
    fn repeated_table_header_reuses_its_shaped_list_marker() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut marker_only = list_paragraph("", 0, true, "");
        marker_only.props.spacing = Spacing {
            before_pt: Some(4.0),
            after_pt: Some(6.0),
            ..Spacing::default()
        };
        let mut header_rows = laid_out_table_rows(
            &Table {
                rows: vec![Row {
                    cells: vec![Cell {
                        blocks: vec![Block::Paragraph(marker_only)],
                        ..Cell::default()
                    }],
                }],
                header_rows: 1,
                ..Table::default()
            },
            geom,
        );
        let pagination = paginate(
            vec![
                pagination_line(30.0),
                FlowItem::Table {
                    rows: vec![header_rows.remove(0), pagination_table_row(false, 4)],
                    header_rows: 1,
                },
            ],
            geom,
            &SectionSetup::default(),
        );

        let rendered_headers: Vec<_> = pagination
            .pages
            .iter()
            .flat_map(|page| page.iter())
            .filter_map(|placed| match &placed.item {
                FlowItem::Row(row) => row
                    .cells
                    .first()
                    .and_then(|cell| cell.lines.first())
                    .map(|line| (shaped_line_text(line), line.cell_spacing, row.height)),
                _ => None,
            })
            .filter(|(text, _, _)| text == "1. ")
            .collect();
        assert!(rendered_headers.len() > 1, "{rendered_headers:?}");
        let expected_height = rendered_headers[0].2;
        assert!(
            rendered_headers.iter().all(|(text, spacing, height)| {
                text == "1. "
                    && *spacing
                        == super::CellLineSpacing {
                            before: 4.0,
                            after: 6.0,
                        }
                    && (*height - expected_height).abs() < 0.001
            }),
            "{rendered_headers:?}"
        );
    }

    fn page_line_counts(pagination: &super::Pagination) -> Vec<usize> {
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

    fn first_page_line_tops(pagination: &super::Pagination) -> Vec<f32> {
        pagination.pages[0]
            .iter()
            .filter_map(|placed| matches!(placed.item, FlowItem::Line(_)).then_some(placed.top))
            .collect()
    }

    #[test]
    fn top_and_bottom_band_moves_overlapping_lines_below_shape() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let items = vec![
            pagination_block(0, PaginationHint::default()),
            FlowItem::TopBottomBand {
                top: 40.0,
                bottom: 60.0,
                anchor_offset: 5,
            },
            pagination_line_with_range(10.0, 0, 10),
            pagination_line(10.0),
            pagination_line(10.0),
        ];

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(first_page_line_tops(&pagination), vec![20.0, 30.0, 60.0]);
        assert_eq!(pagination.pages.len(), 1);
    }

    #[test]
    fn top_and_bottom_band_does_not_reflow_content_before_its_anchor() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let items = vec![
            pagination_block(0, PaginationHint::default()),
            pagination_line(10.0),
            pagination_line(10.0),
            pagination_line(10.0),
            pagination_block(1, PaginationHint::default()),
            FlowItem::TopBottomBand {
                top: 40.0,
                bottom: 70.0,
                anchor_offset: 0,
            },
            pagination_line_with_range(10.0, 0, 5),
            pagination_line(10.0),
        ];

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(
            first_page_line_tops(&pagination),
            vec![20.0, 30.0, 40.0, 50.0, 70.0]
        );
    }

    #[test]
    fn top_and_bottom_band_moves_only_post_anchor_overflow_to_another_page() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut items = vec![
            pagination_block(0, PaginationHint::default()),
            FlowItem::TopBottomBand {
                top: 50.0,
                bottom: 75.0,
                anchor_offset: 0,
            },
            pagination_line_with_range(10.0, 0, 5),
        ];
        items.extend((0..3).map(|_| pagination_line(10.0)));

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(first_page_line_tops(&pagination), vec![20.0, 30.0, 40.0]);
        assert_eq!(page_line_counts(&pagination), vec![3, 1]);
    }

    #[test]
    fn top_and_bottom_band_follows_an_anchor_whose_first_line_advances() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let items = vec![
            pagination_block(0, PaginationHint::default()),
            pagination_line(50.0),
            pagination_block(1, PaginationHint::default()),
            FlowItem::TopBottomBand {
                top: 20.0,
                bottom: 50.0,
                anchor_offset: 0,
            },
            pagination_line_with_range(20.0, 0, 5),
            pagination_line(20.0),
        ];

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(pagination.pages.len(), 2);
        assert_eq!(page_line_counts(&pagination), vec![1, 2]);
        assert_eq!(pagination.pages[1][0].top, 20.0);
        assert_eq!(pagination.pages[1][1].top, 50.0);
        assert_eq!(pagination.block_pages.get(&1), Some(&1));
    }

    #[test]
    fn top_and_bottom_band_preserves_keep_lines_and_widow_control() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 110.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let keep_lines_items = vec![
            pagination_block(0, PaginationHint::default()),
            FlowItem::TopBottomBand {
                top: 35.0,
                bottom: 75.0,
                anchor_offset: 0,
            },
            pagination_line_with_range(10.0, 0, 5),
            pagination_block(
                1,
                PaginationHint {
                    keep_lines: true,
                    ..PaginationHint::default()
                },
            ),
            pagination_line(10.0),
            pagination_line(10.0),
            pagination_line(10.0),
        ];
        let keep_lines = paginate(keep_lines_items, geom, &SectionSetup::default());
        assert_eq!(page_line_counts(&keep_lines), vec![1, 3]);
        assert_eq!(keep_lines.block_pages.get(&1), Some(&1));

        let mut widow_items = vec![
            pagination_block(0, PaginationHint::default()),
            FlowItem::TopBottomBand {
                top: 35.0,
                bottom: 75.0,
                anchor_offset: 0,
            },
            pagination_line_with_range(10.0, 0, 5),
            pagination_block(
                1,
                PaginationHint {
                    widow_control: true,
                    ..PaginationHint::default()
                },
            ),
        ];
        widow_items.extend((0..4).map(|_| pagination_line(10.0)));
        let widow = paginate(widow_items, geom, &SectionSetup::default());
        assert_eq!(page_line_counts(&widow), vec![1, 4]);
        assert_eq!(widow.block_pages.get(&1), Some(&1));
    }

    #[test]
    fn top_and_bottom_band_defers_through_keep_next_chain() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 110.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let items = vec![
            pagination_block(
                0,
                PaginationHint {
                    keep_next: true,
                    ..PaginationHint::default()
                },
            ),
            FlowItem::TopBottomBand {
                top: 35.0,
                bottom: 75.0,
                anchor_offset: 0,
            },
            pagination_line_with_range(10.0, 0, 5),
            pagination_block(1, PaginationHint::default()),
            pagination_line(10.0),
            pagination_block(2, PaginationHint::default()),
            pagination_line(10.0),
        ];

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(first_page_line_tops(&pagination), vec![20.0, 30.0, 75.0]);
    }

    #[test]
    fn top_and_bottom_band_uses_shared_anchor_boundaries_and_page_scope() {
        let range = super::LineCharRange { start: 2, end: 5 };
        assert!(range.contains(2));
        assert!(range.contains(5));
        assert!(!range.contains(6));

        let bands = [super::ActiveTopBottomBand {
            owner_block: Some(3),
            page_index: 1,
            top: 40.0,
            bottom: 60.0,
        }];
        assert_eq!(
            super::top_bottom_adjusted_y(45.0, 10.0, 1, &bands, None),
            60.0
        );
        assert_eq!(
            super::top_bottom_adjusted_y(45.0, 10.0, 0, &bands, None),
            45.0
        );
        assert_eq!(
            super::top_bottom_adjusted_y(45.0, 10.0, 1, &bands, Some(3)),
            45.0
        );
    }

    #[test]
    fn top_and_bottom_bands_require_bounded_page_geometry() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let model = DocModel {
            blocks: vec![para("anchor", None), para("following", None)],
            setup: crate::model::DocSetup {
                page: PageSetup {
                    width_pt: 220.0,
                    height_pt: 100.0,
                    margin_pt: 20.0,
                    ..PageSetup::default()
                },
                ..Default::default()
            },
            ..DocModel::default()
        };
        let shape = FloatingShape {
            id: "bounded-wrap".to_string(),
            name: None,
            description: None,
            text: None,
            preset_geometry: None,
            fill_color: None,
            outline_color: None,
            simple_position_enabled: Some(false),
            simple_position: None,
            effect_extent: Some(ShapeEffectExtent {
                left_emu: 0,
                top_emu: 12_700,
                right_emu: 0,
                bottom_emu: 25_400,
            }),
            anchor_block_index: Some(0),
            anchor_text: Some("anchor".to_string()),
            anchor_char_offset: Some(0),
            extent: Some(ShapeExtent {
                cx_emu: 254_000,
                cy_emu: 254_000,
            }),
            horizontal_position: None,
            vertical_position: Some(ShapePosition {
                relative_from: Some("page".to_string()),
                offset_emu: Some(508_000),
                align: None,
            }),
            relative_height: None,
            behind_doc: Some(false),
            layout_in_cell: Some(false),
            locked: None,
            allow_overlap: None,
            distance: crate::ShapeDistance::default(),
            wrapping: Some(crate::ShapeWrapping {
                kind: "topAndBottom".to_string(),
                text: None,
                distance: crate::ShapeDistance {
                    top_emu: Some(38_100),
                    bottom_emu: Some(50_800),
                    left_emu: None,
                    right_emu: None,
                },
                polygon: Vec::new(),
            }),
        };

        let bands = super::top_bottom_bands_by_block(&model, std::slice::from_ref(&shape), geom);
        assert_eq!(bands.len(), 2);
        assert!((bands[0][0].top - 36.0).abs() < 0.01);
        assert!((bands[0][0].bottom - 66.0).abs() < 0.01);

        let mut simple_position = shape.clone();
        simple_position.simple_position_enabled = Some(true);
        simple_position.simple_position = Some(ShapePoint {
            x_emu: 0,
            y_emu: 508_000,
        });
        simple_position.vertical_position = Some(ShapePosition {
            relative_from: Some("paragraph".to_string()),
            offset_emu: Some(0),
            align: None,
        });
        let simple_position_bands =
            super::top_bottom_bands_by_block(&model, &[simple_position], geom);
        assert!((simple_position_bands[0][0].top - 36.0).abs() < 0.01);
        assert!((simple_position_bands[0][0].bottom - 66.0).abs() < 0.01);

        let mut negative_distances = shape.clone();
        negative_distances
            .wrapping
            .as_mut()
            .unwrap()
            .distance
            .top_emu = Some(-38_100);
        negative_distances
            .wrapping
            .as_mut()
            .unwrap()
            .distance
            .bottom_emu = Some(-50_800);
        let negative_distance_bands =
            super::top_bottom_bands_by_block(&model, &[negative_distances], geom);
        assert!((negative_distance_bands[0][0].top - 39.0).abs() < 0.01);
        assert!((negative_distance_bands[0][0].bottom - 62.0).abs() < 0.01);

        let mut tiny_extent = shape.clone();
        tiny_extent.extent = Some(ShapeExtent {
            cx_emu: 12_700,
            cy_emu: 12_700,
        });
        tiny_extent.effect_extent = None;
        tiny_extent.distance = crate::ShapeDistance::default();
        tiny_extent.wrapping.as_mut().unwrap().distance = crate::ShapeDistance::default();
        let tiny_extent_bands = super::top_bottom_bands_by_block(&model, &[tiny_extent], geom);
        assert!((tiny_extent_bands[0][0].top - 40.0).abs() < 0.01);
        assert!((tiny_extent_bands[0][0].bottom - 41.0).abs() < 0.01);

        let mut font_cx = FontContext::default();
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let mut capture = LayoutCapture::default();
        let items = super::collect_pdf_flow_items(
            &model,
            geom,
            &mut tcx,
            &mut capture,
            super::SourceRenderHints::default(),
            std::slice::from_ref(&shape),
            None,
        );
        assert!(matches!(items[0], FlowItem::BlockStart { index: 0, .. }));
        assert!(matches!(
            items[1],
            FlowItem::TopBottomBand { top, bottom, anchor_offset: 0 }
                if (top - 36.0).abs() < 0.01 && (bottom - 66.0).abs() < 0.01
        ));
        let wrapped_pagination = paginate(items, geom, &SectionSetup::default());
        assert_eq!(wrapped_pagination.block_pages.get(&0), Some(&0));
        assert_eq!(wrapped_pagination.block_pages.get(&1), Some(&1));

        let Block::Paragraph(mut page_break_paragraph) = para("wrapped", None) else {
            unreachable!()
        };
        page_break_paragraph.props.page_break_before = true;
        let page_break_model = DocModel {
            blocks: vec![para("seed", None), Block::Paragraph(page_break_paragraph)],
            setup: model.setup.clone(),
            ..DocModel::default()
        };
        let mut page_break_shape = shape.clone();
        page_break_shape.anchor_block_index = Some(1);
        let mut capture = LayoutCapture::default();
        let page_break_items = super::collect_pdf_flow_items(
            &page_break_model,
            geom,
            &mut tcx,
            &mut capture,
            super::SourceRenderHints::default(),
            &[page_break_shape],
            None,
        );
        let anchor = page_break_items
            .iter()
            .position(|item| matches!(item, FlowItem::BlockStart { index: 1, .. }))
            .unwrap();
        assert!(matches!(page_break_items[anchor + 1], FlowItem::PageBreak));
        assert!(matches!(
            page_break_items[anchor + 2],
            FlowItem::TopBottomBand { .. }
        ));

        let mut paragraph_relative = shape.clone();
        paragraph_relative.vertical_position = Some(ShapePosition {
            relative_from: Some("paragraph".to_string()),
            offset_emu: Some(0),
            align: None,
        });
        let mut behind_text = shape.clone();
        behind_text.behind_doc = Some(true);
        let mut top_margin_relative = shape.clone();
        top_margin_relative.vertical_position = Some(ShapePosition {
            relative_from: Some("topMargin".to_string()),
            offset_emu: Some(0),
            align: None,
        });
        let mut bottom_margin_relative = shape.clone();
        bottom_margin_relative.vertical_position = Some(ShapePosition {
            relative_from: Some("bottomMargin".to_string()),
            offset_emu: Some(0),
            align: None,
        });
        let top_margin_bands = super::top_bottom_bands_by_block(
            &model,
            std::slice::from_ref(&top_margin_relative),
            geom,
        );
        assert!((top_margin_bands[0][0].top - 20.0).abs() < 0.01);
        assert!((top_margin_bands[0][0].bottom - 26.0).abs() < 0.01);
        let bottom_margin_bands = super::top_bottom_bands_by_block(
            &model,
            std::slice::from_ref(&bottom_margin_relative),
            geom,
        );
        assert!((bottom_margin_bands[0][0].top - 76.0).abs() < 0.01);
        assert!((bottom_margin_bands[0][0].bottom - 80.0).abs() < 0.01);

        for mut margin_contained in [top_margin_relative, bottom_margin_relative] {
            margin_contained.extent = Some(ShapeExtent {
                cx_emu: 127_000,
                cy_emu: 127_000,
            });
            margin_contained.effect_extent = None;
            margin_contained.distance = crate::ShapeDistance::default();
            margin_contained.wrapping.as_mut().unwrap().distance = crate::ShapeDistance::default();
            assert!(
                super::top_bottom_bands_by_block(&model, &[margin_contained], geom)[0].is_empty()
            );
        }
        let mut missing_anchor_offset = shape.clone();
        missing_anchor_offset.anchor_char_offset = None;
        let mut layout_in_cell_flag = shape.clone();
        layout_in_cell_flag.layout_in_cell = Some(true);
        assert!(
            !super::top_bottom_bands_by_block(&model, &[layout_in_cell_flag], geom)[0].is_empty()
        );
        let mut square = shape;
        square.wrapping.as_mut().unwrap().kind = "square".to_string();
        for unsupported in [
            paragraph_relative,
            behind_text,
            missing_anchor_offset,
            square,
        ] {
            assert!(super::top_bottom_bands_by_block(&model, &[unsupported], geom)[0].is_empty());
        }
    }

    #[test]
    fn keep_lines_moves_a_bounded_paragraph_to_a_fresh_page() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let items = vec![
            pagination_block(0, PaginationHint::default()),
            pagination_line(40.0),
            pagination_block(
                1,
                PaginationHint {
                    keep_lines: true,
                    ..PaginationHint::default()
                },
            ),
            pagination_line(10.0),
            pagination_line(10.0),
            pagination_line(10.0),
        ];

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(page_line_counts(&pagination), vec![1, 3]);
        assert_eq!(pagination.block_pages.get(&1), Some(&1));
    }

    #[test]
    fn keep_next_moves_the_chain_when_the_following_first_line_would_split() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let items = vec![
            pagination_block(0, PaginationHint::default()),
            pagination_line(40.0),
            pagination_block(
                1,
                PaginationHint {
                    keep_next: true,
                    ..PaginationHint::default()
                },
            ),
            pagination_line(10.0),
            FlowItem::Gap(4.0),
            pagination_block(2, PaginationHint::default()),
            pagination_line(10.0),
        ];

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(page_line_counts(&pagination), vec![1, 2]);
        assert_eq!(pagination.block_pages.get(&1), Some(&1));
        assert_eq!(pagination.block_pages.get(&2), Some(&1));
    }

    #[test]
    fn keep_next_chains_consecutive_paragraphs_as_one_bounded_group() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let keep_next = PaginationHint {
            keep_next: true,
            ..PaginationHint::default()
        };
        let items = vec![
            pagination_block(0, PaginationHint::default()),
            pagination_line(30.0),
            pagination_block(1, keep_next),
            pagination_line(10.0),
            FlowItem::Gap(4.0),
            pagination_block(2, keep_next),
            pagination_line(10.0),
            FlowItem::Gap(4.0),
            pagination_block(3, PaginationHint::default()),
            pagination_line(10.0),
        ];

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(page_line_counts(&pagination), vec![1, 3]);
        assert_eq!(pagination.block_pages.get(&1), Some(&1));
        assert_eq!(pagination.block_pages.get(&2), Some(&1));
        assert_eq!(pagination.block_pages.get(&3), Some(&1));
    }

    #[test]
    fn widow_control_avoids_single_lines_at_both_page_edges() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut items = vec![
            pagination_block(0, PaginationHint::default()),
            pagination_line(30.0),
            pagination_block(
                1,
                PaginationHint {
                    widow_control: true,
                    ..PaginationHint::default()
                },
            ),
        ];
        items.extend((0..4).map(|_| pagination_line(10.0)));

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(page_line_counts(&pagination), vec![3, 2]);
    }

    #[test]
    fn disabled_widow_control_keeps_the_legacy_split() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut items = vec![
            pagination_block(0, PaginationHint::default()),
            pagination_line(50.0),
            pagination_block(1, PaginationHint::default()),
        ];
        items.extend((0..4).map(|_| pagination_line(10.0)));

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(page_line_counts(&pagination), vec![2, 3]);
    }

    #[test]
    fn widow_control_moves_a_single_bottom_line_with_the_paragraph() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut items = vec![
            pagination_block(0, PaginationHint::default()),
            pagination_line(50.0),
            pagination_block(
                1,
                PaginationHint {
                    widow_control: true,
                    ..PaginationHint::default()
                },
            ),
        ];
        items.extend((0..4).map(|_| pagination_line(10.0)));

        let pagination = paginate(items, geom, &SectionSetup::default());

        assert_eq!(page_line_counts(&pagination), vec![1, 4]);
    }

    #[test]
    fn automatically_created_pages_keep_their_section_setup() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let line = || {
            FlowItem::Line(LineLayout {
                height: 20.0,
                baseline: 15.0,
                x_indent: 0.0,
                char_range: None,
                background: None,
                cell_spacing: Default::default(),
                cell_paragraph: None,
                cell_cant_split_group: None,
                runs: Vec::new(),
            })
        };
        let first = SectionSetup {
            header: vec![para("first section", None)],
            ..SectionSetup::default()
        };
        let final_setup = SectionSetup {
            header: vec![para("final section", None)],
            ..SectionSetup::default()
        };
        let mut items = (0..5).map(|_| line()).collect::<Vec<_>>();
        items.push(FlowItem::SectionBreak(first));

        let pagination = paginate(items, geom, &final_setup);

        assert_eq!(pagination.pages.len(), 3);
        assert_eq!(
            block_text(&pagination.page_sections[0].as_ref().unwrap().setup.header),
            "first section"
        );
        assert_eq!(
            block_text(&pagination.page_sections[1].as_ref().unwrap().setup.header),
            "first section"
        );
        assert_eq!(
            block_text(&pagination.page_sections[2].as_ref().unwrap().setup.header),
            "final section"
        );
    }

    #[test]
    fn body_paragraphs_shape_to_their_section_column_width() {
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let page = PageSetup {
            width_pt: 220.0,
            height_pt: 100.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        };
        let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi";
        let mut model = DocModel {
            blocks: vec![para(text, None)],
            setup: crate::model::DocSetup {
                page,
                ..Default::default()
            },
            ..DocModel::default()
        };
        let geom = Geom::from_setup(&page);
        let mut capture = LayoutCapture::default();
        let full_width = super::collect_pdf_flow_items(
            &model,
            geom,
            &mut tcx,
            &mut capture,
            super::SourceRenderHints::default(),
            &[],
            None,
        );
        let full_width_lines = full_width
            .iter()
            .filter(|item| matches!(item, FlowItem::Line(_)))
            .count();

        model.setup.columns = Some(2);
        let mut capture = LayoutCapture::default();
        let columns = super::collect_pdf_flow_items(
            &model,
            geom,
            &mut tcx,
            &mut capture,
            super::SourceRenderHints::default(),
            &[],
            None,
        );
        let column_lines = columns
            .iter()
            .filter(|item| matches!(item, FlowItem::Line(_)))
            .count();
        let setup = SectionSetup::from(&model.setup);
        let pagination = paginate(columns, geom, &setup);

        assert!(column_lines > full_width_lines);
        assert!(pagination.pages[0]
            .iter()
            .any(|placed| matches!(&placed.item, FlowItem::Line(_)) && placed.x > 90.0));
    }

    fn block_text(blocks: &[Block]) -> String {
        blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn line_char_range(line: &super::LineLayout, text: &str) -> Option<(usize, usize)> {
        let mut start = usize::MAX;
        let mut end = 0usize;
        for run in &line.runs {
            for glyph in &run.glyphs {
                start = start.min(glyph.text_range.start);
                end = end.max(glyph.text_range.end);
            }
        }
        (start != usize::MAX).then(|| (text[..start].chars().count(), text[..end].chars().count()))
    }

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

    fn list_paragraph(text: &str, level: u8, ordered: bool, label: &str) -> Paragraph {
        Paragraph {
            props: ParaProps {
                list: Some(ListInfo {
                    level,
                    ordered,
                    label: label.to_string(),
                }),
                ..ParaProps::default()
            },
            runs: (!text.is_empty())
                .then(|| Run {
                    text: text.to_string(),
                    ..Run::default()
                })
                .into_iter()
                .collect(),
        }
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
    fn opened_document_features_add_placeholder_content_to_pdf() {
        let model = DocModel {
            blocks: vec![para("body", None)],
            ..DocModel::default()
        };
        let plain = super::to_pdf(&model);
        let rendered = super::try_to_pdf_with_fonts_and_report(
            &model,
            &[],
            FeatureInventory {
                floating_shapes: 1,
                ..FeatureInventory::default()
            },
        )
        .expect("render with placeholders");

        assert!(rendered.pdf.starts_with(b"%PDF"));
        assert!(
            rendered.pdf.len() > plain.len(),
            "placeholder content should increase emitted PDF size"
        );
        assert_eq!(rendered.report.unsupported.floating_shapes, 1);
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
    fn modeled_table_border_paint_changes_deterministic_pdf_output() {
        let table = Table {
            rows: vec![Row {
                cells: vec![cell("border paint")],
            }],
            ..Table::default()
        };
        let default_model = DocModel {
            blocks: vec![Block::Table(table.clone())],
            ..DocModel::default()
        };
        let border_color = Color::rgb(0xC4, 0x21, 0x32);
        let color_model = DocModel {
            blocks: vec![Block::Table(Table {
                border_color: Some(border_color),
                ..table.clone()
            })],
            ..DocModel::default()
        };
        let width_model = DocModel {
            blocks: vec![Block::Table(Table {
                border_size_eighths: Some(24),
                ..table.clone()
            })],
            ..DocModel::default()
        };
        let styled_model = DocModel {
            blocks: vec![Block::Table(Table {
                border_color: Some(border_color),
                border_size_eighths: Some(24),
                ..table
            })],
            ..DocModel::default()
        };

        let default_pdf = super::to_pdf(&default_model);
        let color_pdf = super::to_pdf(&color_model);
        let width_pdf = super::to_pdf(&width_model);
        let styled_pdf = super::to_pdf(&styled_model);

        assert!(
            color_pdf != default_pdf,
            "border color must affect PDF bytes"
        );
        assert!(
            width_pdf != default_pdf,
            "border width must affect PDF bytes"
        );
        assert!(
            styled_pdf != default_pdf,
            "modeled table border paint must affect PDF bytes"
        );
        assert_eq!(styled_pdf, super::to_pdf(&styled_model));
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        assert_eq!(
            super::layout_pages_with_fonts(&styled_model, &fonts).expect("styled layout pages"),
            super::layout_pages_with_fonts(&default_model, &fonts).expect("default layout pages")
        );

        let mut hostile_model = styled_model;
        let Block::Table(table) = &mut hostile_model.blocks[0] else {
            panic!("table block")
        };
        table.border_size_eighths = Some(u16::MAX);
        let hostile_pdf = super::try_to_pdf(&hostile_model).expect("hostile width stays bounded");
        assert!(hostile_pdf.starts_with(b"%PDF"));
        assert!(hostile_pdf.len() < 5_000_000);
        assert_eq!(hostile_pdf, super::to_pdf(&hostile_model));
    }

    #[test]
    fn six_way_table_border_paint_changes_pdf_without_changing_layout() {
        let table = Table {
            rows: vec![
                Row {
                    cells: vec![cell("A"), cell("B")],
                },
                Row {
                    cells: vec![cell("C"), cell("D")],
                },
            ],
            ..Table::default()
        };
        let baseline_model = DocModel {
            blocks: vec![Block::Table(table.clone())],
            ..DocModel::default()
        };
        let baseline_pdf = super::to_pdf(&baseline_model);
        let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
        let baseline_pages =
            super::layout_pages_with_fonts(&baseline_model, &fonts).expect("baseline pages");

        for (index, side) in [
            TableBorderSide::Top,
            TableBorderSide::Left,
            TableBorderSide::Bottom,
            TableBorderSide::Right,
            TableBorderSide::InsideHorizontal,
            TableBorderSide::InsideVertical,
        ]
        .into_iter()
        .enumerate()
        {
            let mut variant = table.clone();
            variant
                .border_colors
                .set(side, Color::rgb(0x31 + index as u8, 0x72, 0xB4));
            variant.border_sizes.set(side, 16 + index as u16);
            let model = DocModel {
                blocks: vec![Block::Table(variant)],
                ..DocModel::default()
            };
            let pdf = super::to_pdf(&model);

            assert_ne!(pdf, baseline_pdf, "side={side:?}");
            assert_eq!(pdf, super::to_pdf(&model), "side={side:?}");
            assert_eq!(
                super::layout_pages_with_fonts(&model, &fonts).expect("variant pages"),
                baseline_pages,
                "side={side:?}"
            );
        }
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
            ..Default::default()
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

    fn shaped_line_text(line: &LineLayout) -> String {
        line.runs
            .first()
            .map(|run| run.text.to_string())
            .unwrap_or_default()
    }

    fn collected_block_line_texts(blocks: &[Block]) -> Vec<String> {
        let fonts = vec![
            rwml_fonts::noto_sans_kr_subset_with_hanja().to_vec(),
            rwml_fonts::noto_sans_arabic_subset().to_vec(),
            rwml_fonts::noto_sans_hebrew_subset().to_vec(),
        ];
        let mut font_cx = strict_font_context(&fonts);
        let mut layout_cx: LayoutContext<rgb::Color> = LayoutContext::new();
        let mut font_cache = HashMap::new();
        let mut tcx = TextCx {
            font_cx: &mut font_cx,
            layout_cx: &mut layout_cx,
            font_cache: &mut font_cache,
        };
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 300.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let mut flow = Vec::new();
        let mut capture = LayoutCapture::default();
        super::collect_blocks(blocks, &mut flow, geom, &mut tcx, &mut capture);

        let mut texts = Vec::new();
        for item in flow {
            match item {
                FlowItem::Line(line) => texts.push(shaped_line_text(&line)),
                FlowItem::Table { rows, .. } => {
                    for row in rows {
                        for cell in row.cells {
                            texts.extend(cell.lines.iter().map(shaped_line_text));
                        }
                    }
                }
                _ => {}
            }
        }
        texts
    }

    #[test]
    fn table_cell_list_markers_follow_story_order_through_nested_tables() {
        let nested = Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: vec![Block::Paragraph(list_paragraph("nested", 0, true, ""))],
                    ..Cell::default()
                }],
            }],
            ..Table::default()
        };
        let table = Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: vec![
                        Block::Paragraph(list_paragraph("direct", 0, true, "")),
                        Block::Table(nested),
                    ],
                    ..Cell::default()
                }],
            }],
            ..Table::default()
        };
        let blocks = vec![
            Block::Paragraph(list_paragraph("body", 0, true, "")),
            Block::Table(table),
            Block::Paragraph(list_paragraph("tail", 0, true, "")),
        ];

        assert_eq!(
            collected_block_line_texts(&blocks),
            ["1. body", "2. direct", "3. nested", "4. tail"]
        );
    }

    #[test]
    fn table_cell_shaping_omits_unused_source_character_ranges() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let rows = laid_out_table_rows(
            &Table {
                rows: vec![Row {
                    cells: vec![Cell {
                        blocks: vec![Block::Paragraph(list_paragraph(
                            "source range sidecar",
                            0,
                            true,
                            "",
                        ))],
                        ..Cell::default()
                    }],
                }],
                ..Table::default()
            },
            geom,
        );
        let lines = &rows[0].cells[0].lines;

        assert!(!lines.is_empty());
        assert!(lines.iter().all(|line| line.char_range.is_none()));
    }

    #[test]
    fn list_item_beyond_cell_depth_limit_does_not_consume_story_counter() {
        let mut cell = Cell {
            blocks: vec![Block::Paragraph(list_paragraph(
                "beyond depth",
                0,
                true,
                "",
            ))],
            ..Cell::default()
        };
        for _ in 0..=super::MAX_CELL_DEPTH {
            cell = Cell {
                blocks: vec![Block::Table(Table {
                    rows: vec![Row { cells: vec![cell] }],
                    ..Table::default()
                })],
                ..Cell::default()
            };
        }
        let blocks = vec![
            Block::Paragraph(list_paragraph("body", 0, true, "")),
            Block::Table(Table {
                rows: vec![Row { cells: vec![cell] }],
                ..Table::default()
            }),
            Block::Paragraph(list_paragraph("tail", 0, true, "")),
        ];

        assert_eq!(collected_block_line_texts(&blocks), ["1. body", "2. tail"]);
    }

    #[test]
    fn table_cell_lists_prefer_labels_and_render_marker_only_items() {
        let mut heading = list_paragraph("heading", 0, true, "");
        heading.props.heading_level = Some(2);
        let hidden = Paragraph {
            props: ParaProps {
                list: Some(ListInfo {
                    level: 1,
                    ordered: false,
                    label: String::new(),
                }),
                ..ParaProps::default()
            },
            runs: vec![Run {
                text: "SECRET".to_string(),
                props: CharProps {
                    hidden: true,
                    ..CharProps::default()
                },
                ..Run::default()
            }],
        };
        let table = Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: vec![
                        Block::Paragraph(list_paragraph("captured", 0, true, " iv) ")),
                        Block::Paragraph(heading),
                        Block::Paragraph(list_paragraph("fallback", 0, true, "")),
                        Block::Paragraph(hidden),
                        Block::Paragraph(list_paragraph("", 0, true, "")),
                    ],
                    ..Cell::default()
                }],
            }],
            ..Table::default()
        };

        assert_eq!(
            collected_block_line_texts(&[Block::Table(table)]),
            ["iv) captured", "heading", "2. fallback", "◦ ", "3. "]
        );
    }

    #[test]
    fn bidi_visual_cell_list_markers_keep_logical_numbering_order() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 220.0,
            height_pt: 400.0,
            margin_pt: 20.0,
            ..PageSetup::default()
        });
        let cell = |text: &str| {
            let mut paragraph = list_paragraph(text, 0, true, "");
            paragraph.props.align = Align::Right;
            paragraph.props.bidi = true;
            Cell {
                blocks: vec![Block::Paragraph(paragraph)],
                ..Cell::default()
            }
        };
        let rows = laid_out_table_rows(
            &Table {
                rows: vec![Row {
                    cells: vec![cell("أول"), cell("שני")],
                }],
                col_widths_pct: vec![0.5, 0.5],
                bidi_visual: true,
                ..Table::default()
            },
            geom,
        );
        let cells = &rows[0].cells;

        assert!(cells[0].x > cells[1].x);
        assert!(shaped_line_text(&cells[0].lines[0]).starts_with("\u{200f}1. "));
        assert!(shaped_line_text(&cells[1].lines[0]).starts_with("\u{200f}2. "));
        assert!(cells[0].lines[0].runs[0].x > 10.0);
        assert!(cells[1].lines[0].runs[0].x > 10.0);
    }

    fn drawn_line_text(line: &LineLayout) -> String {
        line.runs
            .iter()
            .flat_map(|run| {
                run.glyphs
                    .iter()
                    .filter_map(|glyph| super::glyph_text(&run.text, glyph))
            })
            .collect()
    }

    #[test]
    fn split_table_cell_list_item_injects_one_marker() {
        let geom = Geom::from_setup(&PageSetup {
            width_pt: 170.0,
            height_pt: 180.0,
            margin_pt: 10.0,
            ..PageSetup::default()
        });
        let table = Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: vec![Block::Paragraph(list_paragraph(
                        &"wrapped list content ".repeat(24),
                        0,
                        true,
                        "",
                    ))],
                    ..Cell::default()
                }],
            }],
            ..Table::default()
        };
        let mut rows = laid_out_table_rows(&table, geom);
        let row = rows.remove(0);
        assert!(row.cells[0].lines.len() >= 3);
        let budget = row.cells[0].insets.top + row.cells[0].lines[0].height + f32::EPSILON;
        let (head, tail) = split_row(row, budget);
        let tail = tail.expect("wrapped row should split");
        let drawn = head
            .cells
            .iter()
            .chain(&tail.cells)
            .flat_map(|cell| &cell.lines)
            .map(drawn_line_text)
            .collect::<String>();

        assert_eq!(drawn.matches("1.").count(), 1, "{drawn:?}");
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
    fn renders_hyperlink_without_panicking() {
        use crate::model::FieldRole;
        let model = DocModel {
            blocks: vec![Block::Paragraph(Paragraph {
                props: ParaProps::default(),
                runs: vec![
                    Run {
                        text: "원문 ".to_string(),
                        ..Run::default()
                    },
                    Run {
                        text: "링크".to_string(),
                        field: FieldRole::Hyperlink {
                            url: "https://example.com".to_string(),
                        },
                        ..Run::default()
                    },
                ],
            })],
            ..DocModel::default()
        };
        let pdf = super::to_pdf(&model);
        assert!(pdf.starts_with(b"%PDF"));
        // The URI string is written into the annotation dictionary.
        assert!(
            pdf.windows(b"example.com".len())
                .any(|w| w == b"example.com"),
            "hyperlink URI missing from PDF"
        );
    }

    #[test]
    fn extra_fonts_register_and_garbage_is_ignored() {
        let model = DocModel {
            blocks: vec![para("등록 글꼴 테스트 with 한글", None)],
            ..DocModel::default()
        };
        // Empty and undecodable font blobs must be skipped, not panic; rendering
        // still succeeds via system fonts.
        let pdf = super::to_pdf_with_fonts(&model, &[Vec::new(), vec![1, 2, 3, 4, 5]]);
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 400);
    }

    #[test]
    fn renders_nested_table_cell_text() {
        // A document whose content lives in a table nested inside an outer table's
        // cell must still render its text (not an empty page).
        let inner = Table {
            rows: vec![Row {
                cells: vec![cell("속표 내용"), cell("값")],
            }],
            header_rows: 0,
            ..Default::default()
        };
        let outer = Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: vec![Block::Table(inner)],
                    ..Cell::default()
                }],
            }],
            header_rows: 0,
            ..Default::default()
        };
        let pdf = super::to_pdf(&DocModel {
            blocks: vec![Block::Table(outer)],
            ..DocModel::default()
        });
        assert!(pdf.starts_with(b"%PDF"));
        // The nested cell text must reach the PDF (a glyph-bearing page is larger
        // than an empty one).
        assert!(
            pdf.len() > 1500,
            "nested table text not rendered: {} bytes",
            pdf.len()
        );
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
