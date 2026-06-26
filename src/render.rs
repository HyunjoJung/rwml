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
//! so Korean renders when a Hangul-capable face is installed. For headless/server
//! use without system CJK fonts, a caller can register its own font bytes via
//! [`crate::render_pdf_with_fonts`] (the renderer does not embed a multi-megabyte
//! CJK font into the crate; install one — e.g. Noto Sans CJK — or supply it).

use std::borrow::Cow;
use std::collections::HashMap;
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
use parley::layout::Alignment;
use parley::style::{FontFamily, FontFamilyName, FontStyle, FontWeight, StyleProperty};
use parley::{FontContext, LayoutContext};

use crate::model::{
    Align, Block, Cell, CharProps, Chart, ChartKind, ChartShape, Color, DocModel, FieldRole, Image,
    ListInfo, PageSetup, ParaProps, Paragraph, Run, SectionSetup, Spacing, Table, VCell,
};
use crate::report::{self, FeatureInventory, RenderReport, RenderedPdf};
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
}

const PARA_GAP: f32 = 6.0;
const CELL_PAD: f32 = 3.0;
const BORDER: f32 = 0.4;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DynamicTextKind {
    PageNumber,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DynamicTextRun {
    kind: DynamicTextKind,
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
    /// Hyperlink target, if this run is part of a `FieldRole::Hyperlink` range.
    link: Option<Rc<str>>,
    /// Dynamic text to re-shape when the final page context is known.
    dynamic: Option<DynamicTextRun>,
    text: Rc<str>,
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
    BlockStart(usize),
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
        w: f32,
        h: f32,
    },
    Chart {
        chart: Chart,
        w: f32,
        h: f32,
    },
}

#[derive(Clone)]
struct RenderPageSection {
    setup: SectionSetup,
    first_page_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct FloatingShapeOverlay {
    page_index: usize,
    label: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
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

/// Decode a model image and size it to a [`FlowItem::Picture`] (96-dpi px → PDF
/// points, fit to the content box and a single page height, aspect preserved).
/// `None` if there are no bytes or the format is undecodable.
fn image_flow_item(img: &Image, geom: Geom) -> Option<FlowItem> {
    let bytes = img.bytes.as_ref()?;
    let (image, wpx, hpx) = decode_image(bytes, img.mime.as_deref())?;
    let mut w = wpx as f32 * 0.75;
    let mut h = hpx as f32 * 0.75;
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
    (w > 0.0 && h > 0.0).then_some(FlowItem::Picture { image, w, h })
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
    let black = rgb::Color::new(0, 0, 0);
    // `ranges` are appended in run order, so they are sorted by start and non-overlapping:
    // binary-search the one covering `pos` instead of scanning from the front per cluster,
    // which made shaping O(clusters × runs) = O(N²) on a paragraph of many tiny runs.
    let i = ranges.partition_point(|(s, _, _)| *s <= pos);
    if i == 0 {
        return black;
    }
    let (_, e, p) = &ranges[i - 1];
    if pos < *e {
        p.color
            .map(|c| rgb::Color::new(c.r, c.g, c.b))
            .unwrap_or(black)
    } else {
        black
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

fn display_text(props: &CharProps, text: &str) -> String {
    let text = cased(props, text);
    let Some(font) = props.font.as_deref() else {
        return text;
    };
    let normalized = font
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-' && *ch != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    if normalized.contains("wingdings") {
        map_chars(&text, wingdings_char)
    } else if normalized == "symbol" || normalized.ends_with("symbol") {
        map_chars(&text, symbol_char)
    } else {
        text
    }
}

fn placeholder_label(count: usize, singular: &str, plural: &str, suffix: &str) -> String {
    let label = if count == 1 { singular } else { plural };
    format!("[rdoc preview placeholder: {count} {label} {suffix}]")
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
    if let Some(wrapping) = shape.wrapping.as_ref() {
        layout.push(match wrapping.text.as_deref() {
            Some(text) if !text.trim().is_empty() => {
                format!("wrap {} {}", wrapping.kind, text.trim())
            }
            _ => format!("wrap {}", wrapping.kind),
        });
        if let Some(distance_label) = shape_distance_label("dist", wrapping.distance) {
            if let Some(last) = layout.last_mut() {
                last.push(' ');
                last.push_str(&distance_label);
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
            "margin" | "leftmargin" | "rightmargin" => (geom.left, geom.content_w()),
            _ => (geom.left, geom.content_w()),
        },
        ShapeAxis::Vertical => match relative_from.as_str() {
            "page" => (0.0, geom.page_h),
            "margin" | "topmargin" | "bottommargin" => {
                (geom.top(), (geom.bottom() - geom.top()).max(1.0))
            }
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

fn floating_shape_overlays_for_pages(
    shapes: &[FloatingShape],
    geom: Geom,
    block_pages: &HashMap<usize, usize>,
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
            let page_index = shape
                .anchor_block_index
                .and_then(|index| block_pages.get(&index).copied())
                .unwrap_or(0);
            FloatingShapeOverlay {
                page_index,
                label: floating_shape_label(shape, index, w, h),
                x,
                y,
                w,
                h,
            }
        })
        .collect()
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

fn unsupported_placeholder_blocks(
    features: &FeatureInventory,
    known_floating_shapes: usize,
) -> Vec<Block> {
    unsupported_placeholder_texts_with_known_shapes(features, known_floating_shapes)
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

fn dynamic_text_for_field(field: &FieldRole, props: &CharProps) -> Option<DynamicTextRun> {
    match field {
        FieldRole::Simple { instruction }
            if FieldKind::from_instruction(instruction) == FieldKind::Page =>
        {
            Some(DynamicTextRun {
                kind: DynamicTextKind::PageNumber,
                props: props.clone(),
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
        _ => return None,
    })
}

fn wingdings_char(ch: char) -> Option<char> {
    Some(match ch {
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

/// Shape a styled text string into positioned lines at a given wrap `width`.
#[allow(clippy::too_many_arguments)]
fn shape(
    text: &str,
    ranges: &[(usize, usize, CharProps)],
    links: &[(usize, usize, Rc<str>)],
    dynamic_ranges: &[(usize, usize, DynamicTextRun)],
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
            // Color and hyperlink can change within a single (uniformly-shaped)
            // parley run, so accumulate glyphs into segments, flushing each change.
            let mut seg_color = rgb::Color::new(0, 0, 0);
            let mut seg_link: Option<Rc<str>> = None;
            let mut seg_dynamic: Option<DynamicTextRun> = None;
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
                let lk = link_at(links, cluster.text_range().start);
                let dynamic = dynamic_at(dynamic_ranges, cluster.text_range().start);
                if started
                    && (c != seg_color || lk != seg_link || dynamic != seg_dynamic)
                    && !glyphs.is_empty()
                {
                    runs.push(RunDraw {
                        x: seg_x,
                        glyphs: std::mem::take(&mut glyphs),
                        font: krilla_font.clone(),
                        size: font_size,
                        color: seg_color,
                        link: seg_link.clone(),
                        dynamic: seg_dynamic.clone(),
                        text: text_rc.clone(),
                    });
                    seg_x = x_cursor;
                }
                seg_color = c;
                seg_link = lk;
                seg_dynamic = dynamic;
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
                    link: seg_link,
                    dynamic: seg_dynamic,
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
#[allow(clippy::too_many_arguments)]
fn layout_paragraph(
    p: &Paragraph,
    out: &mut Vec<FlowItem>,
    marker: Option<&str>,
    geom: Geom,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) {
    let list_level = p.props.list.as_ref().map(|l| l.level).unwrap_or(0) as f32;
    let left = p.props.indent.left_pt.unwrap_or(0.0).max(0.0) + list_level * LIST_INDENT;
    let right = p.props.indent.right_pt.unwrap_or(0.0).max(0.0);
    let wrap_w = (geom.content_w() - left - right).max(20.0);

    let mut text = String::new();
    let mut ranges: Vec<(usize, usize, CharProps)> = Vec::new();
    let mut links: Vec<(usize, usize, Rc<str>)> = Vec::new();
    let mut dynamic_ranges: Vec<(usize, usize, DynamicTextRun)> = Vec::new();
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
        text.push_str(&page_field_text(&r.props, &r.text, &r.field, None));
        ranges.push((s, text.len(), r.props.clone()));
        if let FieldRole::Hyperlink { url } = &r.field {
            links.push((s, text.len(), Rc::from(url.as_str())));
        }
        if let Some(dynamic) = dynamic_text_for_field(&r.field, &r.props) {
            dynamic_ranges.push((s, text.len(), dynamic));
        }
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
            &links,
            &dynamic_ranges,
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
        if let Some(item) = image_flow_item(img, geom) {
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
/// ever surfaced. A **nested table** inside a cell is flattened to its cells'
/// lines (no nested grid), recursively — so a document wrapped in an outer table
/// of inner tables still renders its text. Recursion is depth-capped.
fn shape_cell(
    cell: &Cell,
    inner_w: f32,
    depth: u32,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) -> Vec<LineLayout> {
    let mut lines = Vec::new();
    if depth > MAX_CELL_DEPTH {
        return lines;
    }
    for b in &cell.blocks {
        // Bound a pathologically tall cell so the page-split paginator stays linear.
        if lines.len() >= MAX_CELL_LINES {
            break;
        }
        match b {
            Block::Paragraph(p) => {
                let mut text = String::new();
                let mut ranges: Vec<(usize, usize, CharProps)> = Vec::new();
                let mut links: Vec<(usize, usize, Rc<str>)> = Vec::new();
                let mut dynamic_ranges: Vec<(usize, usize, DynamicTextRun)> = Vec::new();
                for r in &p.runs {
                    if r.text.is_empty() {
                        continue;
                    }
                    let s = text.len();
                    text.push_str(&page_field_text(&r.props, &r.text, &r.field, None));
                    ranges.push((s, text.len(), r.props.clone()));
                    if let FieldRole::Hyperlink { url } = &r.field {
                        links.push((s, text.len(), Rc::from(url.as_str())));
                    }
                    if let Some(dynamic) = dynamic_text_for_field(&r.field, &r.props) {
                        dynamic_ranges.push((s, text.len(), dynamic));
                    }
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
                    &links,
                    &dynamic_ranges,
                    p.props.heading_level,
                    align,
                    inner_w,
                    font_cx,
                    layout_cx,
                    font_cache,
                ));
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for c in &row.cells {
                        lines.extend(shape_cell(
                            c,
                            inner_w,
                            depth + 1,
                            font_cx,
                            layout_cx,
                            font_cache,
                        ));
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
#[allow(clippy::too_many_arguments)]
fn layout_table(
    t: &Table,
    out: &mut Vec<FlowItem>,
    geom: Geom,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) {
    let (grid, ncols) = reconstruct_grid(t);
    let content_w = geom.content_w();

    // Column edges: honor authored percentages when they match the grid, else
    // size to content (min 20pt/col) and scale to fill the content width.
    let mut col_x = vec![0.0_f32; ncols + 1];
    if t.col_widths_pct.len() == ncols && t.col_widths_pct.iter().all(|w| *w > 0.0) {
        let sum: f32 = t.col_widths_pct.iter().sum();
        for c in 0..ncols {
            col_x[c + 1] = col_x[c] + content_w * (t.col_widths_pct[c] / sum);
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
        let scale = if total > 0.0 { content_w / total } else { 1.0 };
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
                        0,
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

/// Lay out blocks and keep only the text lines (used for running headers/footers,
/// which are drawn compactly in the page margins; tables/images there are rare and
/// dropped).
fn layout_lines(
    blocks: &[Block],
    geom: Geom,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) -> Vec<LineLayout> {
    let mut items = Vec::new();
    collect_blocks(blocks, &mut items, geom, font_cx, layout_cx, font_cache);
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
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) -> Option<LineLayout> {
    let text = page_number.to_string();
    shape(
        &text,
        &[(0, text.len(), CharProps::default())],
        &[],
        &[],
        None,
        Alignment::Center,
        geom.content_w(),
        font_cx,
        layout_cx,
        font_cache,
    )
    .into_iter()
    .next()
}

fn collect_blocks(
    blocks: &[Block],
    out: &mut Vec<FlowItem>,
    geom: Geom,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) {
    collect_blocks_inner(blocks, out, geom, font_cx, layout_cx, font_cache, false);
}

fn collect_blocks_with_block_anchors(
    blocks: &[Block],
    out: &mut Vec<FlowItem>,
    geom: Geom,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) {
    collect_blocks_inner(blocks, out, geom, font_cx, layout_cx, font_cache, true);
}

fn collect_blocks_inner(
    blocks: &[Block],
    out: &mut Vec<FlowItem>,
    geom: Geom,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
    include_block_anchors: bool,
) {
    let mut lists = ListState::default();
    for (block_index, b) in blocks.iter().enumerate() {
        if include_block_anchors {
            out.push(FlowItem::BlockStart(block_index));
        }
        match b {
            Block::Paragraph(p) => {
                if p.props.page_break_before
                    && out
                        .iter()
                        .any(|item| !matches!(item, FlowItem::BlockStart(_)))
                {
                    out.push(FlowItem::PageBreak);
                }
                // A heading suppresses list marking, mirroring the writer.
                let marker = match (&p.props.list, p.props.heading_level) {
                    (Some(list), None) => Some(lists.marker(list)),
                    _ => None,
                };
                if let Some(before) = p.props.spacing.before_pt.filter(|b| *b > 0.0) {
                    out.push(FlowItem::Gap(before));
                }
                layout_paragraph(
                    p,
                    out,
                    marker.as_deref(),
                    geom,
                    font_cx,
                    layout_cx,
                    font_cache,
                );
                let after = p
                    .props
                    .spacing
                    .after_pt
                    .filter(|a| *a > 0.0)
                    .unwrap_or(PARA_GAP);
                out.push(FlowItem::Gap(after));
            }
            Block::Table(t) => {
                layout_table(t, out, geom, font_cx, layout_cx, font_cache);
                out.push(FlowItem::Gap(PARA_GAP));
            }
            Block::Image(img) => {
                if let Some(item) = image_flow_item(img, geom) {
                    out.push(item);
                    out.push(FlowItem::Gap(PARA_GAP));
                }
            }
            Block::Chart(chart) => {
                if let Some(item) = chart_flow_item(chart, geom) {
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

fn draw_border_color(surface: &mut Surface<'_>, x: f32, y: f32, w: f32, h: f32, color: rgb::Color) {
    fill_rect_color(surface, x, y, w, BORDER, color);
    fill_rect_color(surface, x, y + h - BORDER, w, BORDER, color);
    fill_rect_color(surface, x, y, BORDER, h, color);
    fill_rect_color(surface, x + w - BORDER, y, BORDER, h, color);
}

fn draw_floating_shape_overlay(
    surface: &mut Surface<'_>,
    overlay: &FloatingShapeOverlay,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
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
        7.5,
        false,
        Alignment::Start,
        Color::rgb(0x32, 0x3A, 0x43),
        font_cx,
        layout_cx,
        font_cache,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_chart_text(
    surface: &mut Surface<'_>,
    text: &str,
    x: f32,
    y: f32,
    width: f32,
    size_pt: f32,
    bold: bool,
    align: Alignment,
    color: Color,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
) -> f32 {
    if text.trim().is_empty() || width <= 0.0 {
        return 0.0;
    }
    let size_half_pt = (size_pt * 2.0).round().max(1.0) as u16;
    let props = CharProps {
        bold,
        size_half_pt: Some(size_half_pt),
        color: Some(color),
        ..CharProps::default()
    };
    let mut consumed = 0.0;
    for line in shape(
        text,
        &[(0, text.len(), props)],
        &[],
        &[],
        None,
        align,
        width,
        font_cx,
        layout_cx,
        font_cache,
    )
    .into_iter()
    .take(2)
    {
        let baseline = y + consumed + line.baseline;
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

fn draw_pie_chart(
    surface: &mut Surface<'_>,
    chart: &Chart,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
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
    let radius = (w.min(h) * 0.42).max(1.0);
    let cx = x + w * 0.5;
    let cy = y + h * 0.5;
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

#[allow(clippy::too_many_arguments)]
fn draw_radar_chart(
    surface: &mut Surface<'_>,
    chart: &Chart,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
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
            7.5,
            false,
            Alignment::Center,
            Color::rgb(0x25, 0x2D, 0x36),
            font_cx,
            layout_cx,
            font_cache,
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

#[allow(clippy::too_many_arguments)]
fn draw_authored_chart(
    surface: &mut Surface<'_>,
    chart: &Chart,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
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
            11.0,
            true,
            Alignment::Center,
            Color::rgb(0x1E, 0x2A, 0x36),
            font_cx,
            layout_cx,
            font_cache,
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
            plot_left,
            plot_top,
            plot_w,
            plot_h,
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
                8.0,
                false,
                Alignment::Start,
                Color::rgb(0x25, 0x2D, 0x36),
                font_cx,
                layout_cx,
                font_cache,
            );
            legend_x += 9.0 + (category.chars().count() as f32 * 4.8).max(used * 3.0) + 12.0;
        }
        return;
    }

    if matches!(
        chart.kind,
        ChartKind::Radar | ChartKind::RadarWithMarkers | ChartKind::FilledRadar
    ) {
        draw_radar_chart(
            surface, chart, plot_left, plot_top, plot_w, plot_h, font_cx, layout_cx, font_cache,
        );
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
                8.0,
                false,
                Alignment::Start,
                Color::rgb(0x25, 0x2D, 0x36),
                font_cx,
                layout_cx,
                font_cache,
            );
            legend_x += 9.0 + (series.name.chars().count() as f32 * 4.8).max(used * 3.0) + 12.0;
        }
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
                    7.5,
                    false,
                    Alignment::Center,
                    Color::rgb(0x4C, 0x55, 0x5F),
                    font_cx,
                    layout_cx,
                    font_cache,
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
                    8.0,
                    false,
                    Alignment::End,
                    Color::rgb(0x25, 0x2D, 0x36),
                    font_cx,
                    layout_cx,
                    font_cache,
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
                    7.5,
                    false,
                    Alignment::Center,
                    Color::rgb(0x4C, 0x55, 0x5F),
                    font_cx,
                    layout_cx,
                    font_cache,
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
                    8.0,
                    false,
                    Alignment::End,
                    Color::rgb(0x25, 0x2D, 0x36),
                    font_cx,
                    layout_cx,
                    font_cache,
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
                    7.5,
                    false,
                    Alignment::End,
                    Color::rgb(0x4C, 0x55, 0x5F),
                    font_cx,
                    layout_cx,
                    font_cache,
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
                    8.0,
                    false,
                    Alignment::Center,
                    Color::rgb(0x25, 0x2D, 0x36),
                    font_cx,
                    layout_cx,
                    font_cache,
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
                ChartKind::Column | ChartKind::Column3D => {
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
                            7.5,
                            false,
                            Alignment::End,
                            Color::rgb(0x25, 0x2D, 0x36),
                            font_cx,
                            layout_cx,
                            font_cache,
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
                | ChartKind::ExplodedDoughnut => {}
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
        | ChartKind::ExplodedDoughnut => {}
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
            8.0,
            false,
            Alignment::Start,
            Color::rgb(0x25, 0x2D, 0x36),
            font_cx,
            layout_cx,
            font_cache,
        );
        legend_x += 9.0 + (series.name.chars().count() as f32 * 4.8).max(used * 3.0) + 12.0;
    }
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

#[allow(clippy::too_many_arguments)]
fn draw_run_with_page_context(
    surface: &mut Surface<'_>,
    run: RunDraw,
    x_abs: f32,
    baseline_y: f32,
    page_number: usize,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<rgb::Color>,
    font_cache: &mut HashMap<u64, Font>,
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
        &[(0, text.len(), dynamic.props)],
        &[],
        &[],
        None,
        Alignment::Start,
        1024.0,
        font_cx,
        layout_cx,
        font_cache,
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

type Pages = Vec<Vec<(f32, FlowItem)>>;

struct PdfRender {
    pdf: Vec<u8>,
    pages: usize,
}

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
fn ensure(pages: &mut Pages, y: &mut f32, h: f32, geom: Geom) {
    if *y + h > geom.bottom() && page_nonempty(pages) {
        pages.push(Vec::new());
        *y = geom.top();
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
    geom: Geom,
) {
    let mut on_fresh = !page_nonempty(pages);
    loop {
        let avail = geom.bottom() - *y;
        if row.height <= avail {
            let h = row.height;
            place_item(pages, y, FlowItem::Row(row), h);
            return;
        }
        if !on_fresh {
            // Move the whole row to a fresh page and repeat headers.
            pages.push(Vec::new());
            *y = geom.top();
            if !is_header {
                repeat_headers(pages, y, headers);
            }
            on_fresh = true;
            continue;
        }
        // On a fresh page (after any headers) and still too tall: split.
        let (frag, rest) = split_row(row, geom.bottom() - *y);
        let fh = frag.height;
        place_item(pages, y, FlowItem::Row(frag), fh);
        pages.push(Vec::new());
        *y = geom.top();
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
fn place_table(
    pages: &mut Pages,
    y: &mut f32,
    rows: Vec<RowLayout>,
    header_rows: usize,
    geom: Geom,
) {
    let mut headers: Vec<RowLayout> = rows.iter().take(header_rows).cloned().collect();
    // Only repeat headers that fit a page. A header taller than the content box would overflow
    // on every page (place_item does not split it), forcing each following body row to break to
    // a fresh page and re-clone the whole header — O(rows × header_lines). Dropping the repeat
    // for an over-tall header keeps pagination linear (the header still renders inline once).
    let page_h = geom.bottom() - geom.top();
    if headers.iter().map(|h| h.height).sum::<f32>() > page_h {
        headers.clear();
    }
    for (i, row) in rows.into_iter().enumerate() {
        place_row(pages, y, row, &headers, i < header_rows, geom);
    }
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

/// Render a [`DocModel`] to a single-column A4 PDF using system fonts.
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
    Ok(render_pdf(model, extra_fonts, None, &[])?.pdf)
}

pub(crate) fn to_pdf_with_fonts_and_features_and_shapes(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
    floating_shapes: &[FloatingShape],
) -> Vec<u8> {
    try_to_pdf_with_fonts_and_features_and_shapes(model, extra_fonts, features, floating_shapes)
        .unwrap_or_default()
}

pub(crate) fn try_to_pdf_with_fonts_and_features_and_shapes(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
    floating_shapes: &[FloatingShape],
) -> Result<Vec<u8>> {
    let unsupported = report::render_unsupported_features(&features);
    Ok(render_pdf(model, extra_fonts, Some(&unsupported), floating_shapes)?.pdf)
}

pub(crate) fn to_pdf_with_fonts_and_report(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
) -> RenderedPdf {
    to_pdf_with_fonts_and_report_and_shapes(model, extra_fonts, features, &[])
}

pub(crate) fn to_pdf_with_fonts_and_report_and_shapes(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
    floating_shapes: &[FloatingShape],
) -> RenderedPdf {
    let unsupported = report::render_unsupported_features(&features);
    let fallback_unsupported = unsupported.clone();
    try_to_pdf_with_fonts_and_report_and_shapes(model, extra_fonts, features, floating_shapes)
        .unwrap_or_else(|_| RenderedPdf {
            pdf: Vec::new(),
            report: RenderReport {
                pages: 0,
                warnings: report::render_warnings_for(&fallback_unsupported),
                unsupported: fallback_unsupported,
            },
        })
}

pub(crate) fn try_to_pdf_with_fonts_and_report(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
) -> Result<RenderedPdf> {
    try_to_pdf_with_fonts_and_report_and_shapes(model, extra_fonts, features, &[])
}

pub(crate) fn try_to_pdf_with_fonts_and_report_and_shapes(
    model: &DocModel,
    extra_fonts: &[Vec<u8>],
    features: FeatureInventory,
    floating_shapes: &[FloatingShape],
) -> Result<RenderedPdf> {
    let unsupported = report::render_unsupported_features(&features);
    let rendered = render_pdf(model, extra_fonts, Some(&unsupported), floating_shapes)?;
    let warnings = report::render_warnings_for(&unsupported);
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
    // Page geometry from the document (Letter/A4/A3/landscape/custom margins).
    let geom = Geom::from_setup(&model.setup.page);
    let floating_shape_overlay_count = floating_shapes.len().min(MAX_FLOATING_SHAPE_OVERLAYS);

    let mut items: Vec<FlowItem> = Vec::new();
    collect_blocks_with_block_anchors(
        &model.blocks,
        &mut items,
        geom,
        &mut font_cx,
        &mut layout_cx,
        &mut font_cache,
    );
    if let Some(features) = unsupported_features {
        let placeholders = unsupported_placeholder_blocks(features, floating_shape_overlay_count);
        if !placeholders.is_empty() {
            if !items.is_empty() {
                items.push(FlowItem::Gap(PARA_GAP));
            }
            collect_blocks(
                &placeholders,
                &mut items,
                geom,
                &mut font_cx,
                &mut layout_cx,
                &mut font_cache,
            );
        }
    }
    // Paginate: flow items top-to-bottom onto pages sized by `geom`. Tables repeat
    // their header rows after each break and split rows taller than a page.
    let mut pages: Pages = vec![Vec::new()];
    let mut page_sections: Vec<Option<RenderPageSection>> = vec![None];
    let mut section_start_page_index = 0usize;
    let mut y = geom.top();
    let mut block_pages = HashMap::new();
    let mut pending_block = None;
    for item in items {
        match item {
            FlowItem::BlockStart(block_index) => {
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                pending_block = Some(block_index);
            }
            FlowItem::Gap(g) => y += g,
            FlowItem::Line(l) => {
                let h = l.height;
                ensure(&mut pages, &mut y, h, geom);
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                place_item(&mut pages, &mut y, FlowItem::Line(l), h);
            }
            FlowItem::Picture { image, w, h } => {
                ensure(&mut pages, &mut y, h, geom);
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                place_item(&mut pages, &mut y, FlowItem::Picture { image, w, h }, h);
            }
            FlowItem::Chart { chart, w, h } => {
                ensure(&mut pages, &mut y, h, geom);
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                place_item(&mut pages, &mut y, FlowItem::Chart { chart, w, h }, h);
            }
            FlowItem::Table { rows, header_rows } => {
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                place_table(&mut pages, &mut y, rows, header_rows, geom);
            }
            FlowItem::PageBreak => {
                pages.push(Vec::new());
                page_sections.push(None);
                y = geom.top();
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
            }
            FlowItem::SectionBreak(section) => {
                let section_end_page_index = pages.len().saturating_sub(1);
                assign_section_to_render_pages(
                    &mut page_sections,
                    section_start_page_index,
                    section_end_page_index,
                    &section,
                );
                pages.push(Vec::new());
                page_sections.push(None);
                y = geom.top();
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                section_start_page_index = pages.len().saturating_sub(1);
            }
            // Rows reach pagination only inside a Table; place defensively.
            FlowItem::Row(r) => {
                let h = r.height;
                ensure(&mut pages, &mut y, h, geom);
                record_pending_block_page(
                    &mut block_pages,
                    &mut pending_block,
                    pages.len().saturating_sub(1),
                );
                place_item(&mut pages, &mut y, FlowItem::Row(r), h);
            }
        }
    }
    record_pending_block_page(
        &mut block_pages,
        &mut pending_block,
        pages.len().saturating_sub(1),
    );
    let final_section_setup = SectionSetup::from(&model.setup);
    assign_section_to_render_pages(
        &mut page_sections,
        section_start_page_index,
        pages.len().saturating_sub(1),
        &final_section_setup,
    );
    let floating_shape_overlays =
        floating_shape_overlays_for_pages(floating_shapes, geom, &block_pages);

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
        let header_lines = layout_lines(
            header_blocks,
            geom,
            &mut font_cx,
            &mut layout_cx,
            &mut font_cache,
        );
        let footer_lines = layout_lines(
            footer_blocks,
            geom,
            &mut font_cx,
            &mut layout_cx,
            &mut font_cache,
        );
        let mut surface = page.surface();
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
            for run in &line.runs {
                draw_run_with_page_context(
                    &mut surface,
                    run.clone(),
                    x0,
                    baseline,
                    page_index + 1,
                    &mut font_cx,
                    &mut layout_cx,
                    &mut font_cache,
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
            for run in &line.runs {
                draw_run_with_page_context(
                    &mut surface,
                    run.clone(),
                    x0,
                    baseline,
                    page_index + 1,
                    &mut font_cx,
                    &mut layout_cx,
                    &mut font_cache,
                );
            }
            fy += line.height;
        }
        if page_section.setup.page_numbers {
            if let Some(line) = layout_page_number_line(
                page_index + 1,
                geom,
                &mut font_cx,
                &mut layout_cx,
                &mut font_cache,
            ) {
                if fy + line.height <= geom.page_h {
                    let baseline = fy + line.baseline;
                    let x0 = geom.left + line.x_indent;
                    for run in line.runs {
                        draw_run(&mut surface, run, x0, baseline);
                    }
                }
            }
        }
        for (top, item) in page_items {
            match item {
                FlowItem::BlockStart(_)
                | FlowItem::Gap(_)
                | FlowItem::PageBreak
                | FlowItem::SectionBreak(_)
                | FlowItem::Table { .. } => {}
                FlowItem::Picture { image, w, h } => {
                    // Center horizontally within the content box.
                    let x = geom.left + ((geom.content_w() - w) * 0.5).max(0.0);
                    if let Some(sz) = Size::from_wh(w, h) {
                        surface.push_transform(&Transform::from_translate(x, top));
                        surface.draw_image(image, sz);
                        surface.pop();
                    }
                }
                FlowItem::Chart { chart, w, h } => {
                    let x = geom.left + ((geom.content_w() - w) * 0.5).max(0.0);
                    draw_authored_chart(
                        &mut surface,
                        &chart,
                        x,
                        top,
                        w,
                        h,
                        &mut font_cx,
                        &mut layout_cx,
                        &mut font_cache,
                    );
                }
                FlowItem::Line(line) => {
                    let baseline = top + line.baseline;
                    let x0 = geom.left + line.x_indent;
                    let lh = line.height;
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
                            &mut font_cx,
                            &mut layout_cx,
                            &mut font_cache,
                        );
                    }
                }
                FlowItem::Row(row) => {
                    for cell in row.cells {
                        let cx = geom.left + cell.x;
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
                                if let Some(url) = run.link.clone() {
                                    let l = cx + CELL_PAD + run.x;
                                    page_links.push((l, ly, l + run.width(), ly + lh, url));
                                }
                                draw_run_with_page_context(
                                    &mut surface,
                                    run,
                                    cx + CELL_PAD,
                                    baseline,
                                    page_index + 1,
                                    &mut font_cx,
                                    &mut layout_cx,
                                    &mut font_cache,
                                );
                            }
                            ly += lh;
                        }
                    }
                }
            }
        }
        for overlay in floating_shape_overlays
            .iter()
            .filter(|overlay| overlay.page_index == page_index)
        {
            draw_floating_shape_overlay(
                &mut surface,
                overlay,
                &mut font_cx,
                &mut layout_cx,
                &mut font_cache,
            );
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
    use parley::{FontContext, LayoutContext};
    use std::collections::HashMap;

    use super::{
        assign_section_to_render_pages, display_text, layout_page_number_line, page_field_text,
        rgb, running_header_footer_blocks_for_page, unsupported_placeholder_texts, Geom,
    };
    use crate::model::{
        Block, Cell, CharProps, Color, DocModel, FieldRole, PageSetup, ParaProps, Paragraph, Row,
        Run, SectionSetup, Table,
    };
    use crate::report::FeatureInventory;
    use crate::{FloatingShape, ShapeEffectExtent, ShapeExtent, ShapePoint, ShapePosition};

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
        assert_eq!(display_text(&symbol, "abg"), "αβγ");

        let wingdings = CharProps {
            font: Some("Wingdings".to_string()),
            ..CharProps::default()
        };
        assert_eq!(display_text(&wingdings, "\u{00FC}"), "✓");
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
                "[rdoc preview placeholder: 2 floating shapes preserved but not positioned]",
                "[rdoc preview placeholder: 1 chart preserved but not modeled]",
                "[rdoc preview placeholder: 4 OLE objects preserved but not modeled]",
                "[rdoc preview placeholder: 5 WMF/EMF images preserved but not rendered]",
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
                "[rdoc preview placeholder: 1 floating shape preserved but not positioned]",
                "[rdoc preview placeholder: 1 chart preserved but not modeled]",
            ]
        );
        assert_eq!(
            super::unsupported_placeholder_texts_with_known_shapes(&features, 3),
            vec!["[rdoc preview placeholder: 1 chart preserved but not modeled]"]
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
                distance: crate::ShapeDistance::default(),
                wrapping: Some(crate::ShapeWrapping {
                    kind: "square".to_string(),
                    text: Some("bothSides".to_string()),
                    distance: crate::ShapeDistance {
                        top_emu: Some(9_144),
                        bottom_emu: Some(18_288),
                        left_emu: Some(27_432),
                        right_emu: Some(36_576),
                    },
                }),
            }],
            geom,
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
            "floating shape 1: Float one (72 x 36 pt, x simplePos 14.4 pt, y simplePos 21.6 pt, z 251659264, front, wrap square bothSides dist t 0.7 pt, b 1.4 pt, l 2.2 pt, r 2.9 pt, geometry roundRect, effect l 0.7 pt, t 1.4 pt, r 2.2 pt, b 2.9 pt, fill #FF8800, outline #003366, anchor Before anchor After anchor, text Shape body)"
        );
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
        );

        assert_eq!(overlays.len(), 2);
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

        let line = layout_page_number_line(7, geom, &mut font_cx, &mut layout_cx, &mut font_cache)
            .expect("page number line");
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
