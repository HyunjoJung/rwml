//! The rich document model — a typed intermediate representation built lazily on
//! top of the piece table, character/paragraph properties, the style sheet, and
//! the table structure.
//!
//! The flat [`crate::Document::text`] path is untouched and stays fast; the model
//! is only assembled when a caller asks for [`crate::Document::model`],
//! [`crate::Document::to_markdown`], or [`crate::Document::to_html`].
//!
//! The shape mirrors the de-facto Rust/Pandoc document IRs (a document is a list
//! of block-level nodes; paragraphs hold inline runs) so the Markdown/HTML
//! exporters are simple folds.

use crate::annotation::{NoteKind, RevisionKind};

/// A whole `.doc` document as an ordered list of block-level nodes plus
/// document-level metadata.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocModel {
    /// Block-level content, in reading order.
    pub blocks: Vec<Block>,
    /// Source-region spans for content that came from distinct Word
    /// subdocuments. Empty for authored models and sources that do not expose a
    /// region map yet.
    pub regions: Vec<SourceRegion>,
    /// Document-level metadata (codepage, language, counts).
    pub meta: DocMeta,
    /// Document-level layout for authoring/rendering (page size, header/footer,
    /// metadata). Defaults to A4 portrait with no running header/footer.
    pub setup: DocSetup,
}

/// A coarse source subdocument region from the original Word file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceRegionKind {
    /// Main body text.
    Main,
    /// Legacy footnote subdocument.
    Footnote,
    /// Header/footer subdocument.
    HeaderFooter,
    /// Legacy annotation/comment subdocument.
    Annotation,
    /// Legacy endnote subdocument.
    Endnote,
    /// Text-box subdocument.
    TextBox,
}

/// A span of [`DocModel::blocks`] that came from one source subdocument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRegion {
    /// Region kind.
    pub kind: SourceRegionKind,
    /// Source-specific story index within the subdocument, when a format table
    /// exposes one. For legacy `.doc` header/footer regions this is the
    /// `PlcfHdd` story index.
    pub source_story_index: Option<usize>,
    /// Inclusive start block index in [`DocModel::blocks`].
    pub block_start: usize,
    /// Exclusive end block index in [`DocModel::blocks`].
    pub block_end: usize,
    /// UTF-16 CP start in the original Word source stream.
    pub source_start_cp: usize,
    /// UTF-16 CP length reported by the source metadata.
    pub source_len_cp: usize,
    /// Visible character start within the flattened model text.
    pub text_start: usize,
    /// Visible character length contributed by this region.
    pub text_len: usize,
}

impl DocModel {
    /// Iterate source-region spans of the requested kind.
    pub fn source_regions(&self, kind: SourceRegionKind) -> impl Iterator<Item = &SourceRegion> {
        self.regions
            .iter()
            .filter(move |region| region.kind == kind)
    }

    /// Return the block slice covered by a source-region span.
    ///
    /// The range is clamped to this model, so passing a stale region from another
    /// model returns an empty or shortened slice instead of panicking.
    pub fn source_region_blocks(&self, region: &SourceRegion) -> &[Block] {
        let start = region.block_start.min(self.blocks.len());
        let end = region.block_end.min(self.blocks.len());
        if start <= end {
            &self.blocks[start..end]
        } else {
            &self.blocks[0..0]
        }
    }

    /// Concatenate visible text from the blocks covered by a source-region span.
    pub fn source_region_text(&self, region: &SourceRegion) -> String {
        let mut out = String::new();
        append_blocks_text(self.source_region_blocks(region), &mut out);
        out
    }

    /// Concatenate visible text from all source regions of one kind, in model
    /// order. Returns an empty string when the model has no matching regions.
    pub fn source_region_kind_text(&self, kind: SourceRegionKind) -> String {
        let mut out = String::new();
        for region in self.source_regions(kind) {
            append_blocks_text(self.source_region_blocks(region), &mut out);
        }
        out
    }
}

fn append_blocks_text(blocks: &[Block], out: &mut String) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                for run in &paragraph.runs {
                    out.push_str(&run.text);
                }
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        append_blocks_text(&cell.blocks, out);
                    }
                }
            }
            Block::Image(_) | Block::Chart(_) | Block::SectionBreak(_) => {}
            Block::PageBreak => out.push('\n'),
        }
    }
}

/// Document-level metadata.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocMeta {
    /// ANSI codepage of 8-bit pieces (e.g. 949 for Korean).
    pub codepage: u16,
    /// FIB language id (`lid`).
    pub lid: u16,
    /// Aggregate counts (paragraphs, tables, figures, characters).
    pub stats: Stats,
}

/// Aggregate document statistics, mirroring the project-wide `DocStats` contract.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Number of paragraphs (including headings, list items, and cell paragraphs).
    pub paragraphs: u32,
    /// Number of tables.
    pub tables: u16,
    /// Number of images / figures.
    pub figures: u16,
    /// Total visible character count.
    pub text_chars: usize,
}

/// A block-level node.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A paragraph — also carries headings and list items via [`ParaProps`].
    Paragraph(Paragraph),
    /// A table (rows → cells → blocks).
    Table(Table),
    /// A block-level image (an image-only paragraph).
    Image(Image),
    /// A block-level chart with literal category/value data.
    Chart(Chart),
    /// An explicit page break between block-level items.
    PageBreak,
    /// A section boundary carrying layout for the section that just ended.
    SectionBreak(SectionSetup),
}

/// A paragraph: inline runs plus paragraph-level properties.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Paragraph {
    /// Paragraph-level properties (style, heading, alignment, list membership).
    pub props: ParaProps,
    /// Inline runs, in reading order.
    pub runs: Vec<Run>,
}

impl Paragraph {
    /// The concatenated visible text of all runs (hidden runs included — they are
    /// part of the indexed text; the HTML exporter may choose to omit them).
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// `true` if the paragraph has no visible text and no image in any run.
    pub fn is_blank(&self) -> bool {
        self.runs
            .iter()
            .all(|r| r.text.trim().is_empty() && r.image.is_none())
    }
}

/// Paragraph spacing in points; `None` = unset (renderer/writer uses defaults).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Spacing {
    /// Space above the paragraph.
    pub before_pt: Option<f32>,
    /// Space below the paragraph.
    pub after_pt: Option<f32>,
    /// Line height as a multiple of the font size (e.g. `1.5`).
    pub line_pct: Option<f32>,
}

/// Paragraph indentation in points; `None` = unset.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Indent {
    /// Left indent.
    pub left_pt: Option<f32>,
    /// Right indent.
    pub right_pt: Option<f32>,
    /// First-line additional indent.
    pub first_line_pt: Option<f32>,
    /// Hanging indent (first line outdented by this much).
    pub hanging_pt: Option<f32>,
}

/// Paragraph-level properties.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParaProps {
    /// Raw Word paragraph style id (`w:pStyle/@w:val`), if known or authored.
    pub style_id: Option<String>,
    /// Resolved style name (e.g. `Heading 1`, `제목 1`), if the style sheet was read.
    pub style_name: Option<String>,
    /// Heading level `1..=6`, from the outline level or a recognized style name.
    pub heading_level: Option<u8>,
    /// Paragraph alignment.
    pub align: Align,
    /// Raw outline level `0..=8` (9/None = body text).
    pub outline_level: Option<u8>,
    /// List membership — `Some` makes this paragraph a list item.
    pub list: Option<ListInfo>,
    /// Spacing (before/after/line).
    pub spacing: Spacing,
    /// Indentation.
    pub indent: Indent,
    /// Paragraph background shading, if any.
    pub shading: Option<Color>,
}

/// A paragraph style definition for generated `.docx` output.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParagraphStyle {
    /// Stable Word style id (`w:styleId` / paragraph `w:pStyle` value).
    pub id: String,
    /// Human-readable style name.
    pub name: String,
    /// Base style id, if any.
    pub based_on: Option<String>,
    /// Next paragraph style id, if any.
    pub next: Option<String>,
    /// Whether the style should appear in Word's quick style gallery.
    pub q_format: bool,
    /// Optional heading level `1..=9`, emitted as style `outlineLvl`.
    pub heading_level: Option<u8>,
    /// Default paragraph alignment.
    pub align: Align,
    /// Default paragraph spacing.
    pub spacing: Spacing,
    /// Default paragraph indentation.
    pub indent: Indent,
    /// Default paragraph background shading.
    pub shading: Option<Color>,
    /// Default character properties for runs using this style.
    pub run: CharProps,
}

/// A comment to author on a generated run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthoredComment {
    /// Comment body text.
    pub text: String,
    /// Comment author, if any.
    pub author: Option<String>,
    /// Author initials, if any.
    pub initials: Option<String>,
    /// Comment timestamp, if any.
    pub date: Option<String>,
}

/// Tracked revision metadata to author on a generated run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredRevision {
    /// Revision kind. Generated `.docx` currently supports insertion/deletion.
    pub kind: RevisionKind,
    /// Revision author, if any.
    pub author: Option<String>,
    /// Revision timestamp, if any.
    pub date: Option<String>,
}

/// Plain text content-control metadata to author on a generated run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthoredContentControl {
    /// Human-readable content-control title/alias, if any.
    pub alias: Option<String>,
    /// Machine-readable content-control tag, if any.
    pub tag: Option<String>,
}

/// Footnote or endnote metadata to author after a generated run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredNote {
    /// Footnote or endnote.
    pub kind: NoteKind,
    /// Note body text.
    pub text: String,
}

impl Default for AuthoredRevision {
    fn default() -> Self {
        Self {
            kind: RevisionKind::Insertion,
            author: None,
            date: None,
        }
    }
}

/// List membership of a paragraph.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListInfo {
    /// 0-based nesting level.
    pub level: u8,
    /// `true` = numbered list, `false` = bullet list.
    pub ordered: bool,
    /// The rendered autonumber label (e.g. `1.`, `가.`) — used by the flat text
    /// path; the Markdown/HTML exporters use native list syntax instead.
    pub label: String,
}

/// An inline run of text with uniform character properties.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    /// The run's text.
    pub text: String,
    /// Character formatting.
    pub props: CharProps,
    /// Field role (a hyperlink result carries its URL).
    pub field: FieldRole,
    /// An inline picture (the run's text is empty when this is set).
    pub image: Option<Image>,
    /// Authored comment anchored to this run.
    pub comment: Option<AuthoredComment>,
    /// Authored tracked insertion/deletion metadata for this run.
    pub revision: Option<AuthoredRevision>,
    /// Authored plain text content-control metadata for this run.
    pub content_control: Option<AuthoredContentControl>,
    /// Authored bookmark name wrapping this run.
    pub bookmark: Option<String>,
    /// Authored footnote/endnote anchored after this run.
    pub note: Option<AuthoredNote>,
}

/// An sRGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

impl Color {
    /// Construct from components.
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }
}

/// Vertical alignment of a run (super/subscript).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VertAlign {
    /// On the baseline (normal).
    #[default]
    Baseline,
    /// Superscript.
    Super,
    /// Subscript.
    Sub,
}

/// Character-level formatting that affects rendering.
///
/// `font`/`color`/`highlight` make this non-`Copy`; clone where a `CharProps` is
/// needed by value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CharProps {
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Struck through.
    pub strike: bool,
    /// Hidden text (`fVanish`) — kept in the flat/Markdown text for index recall,
    /// but the HTML exporter omits it.
    pub hidden: bool,
    /// Font family name (emitted to `w:rFonts` ascii+eastAsia), if known.
    pub font: Option<String>,
    /// Font size in half-points (Word unit; `24` = 12pt), if known.
    pub size_half_pt: Option<u16>,
    /// Text color, if known.
    pub color: Option<Color>,
    /// Highlight color name (`w:highlight`, e.g. `"yellow"`), if any.
    pub highlight: Option<String>,
    /// Super/subscript.
    pub vert_align: VertAlign,
    /// Small caps (`w:smallCaps`): lowercase letters render as small capitals.
    pub small_caps: bool,
    /// All caps (`w:caps`): text renders uppercased regardless of stored case.
    pub caps: bool,
}

/// Whether a run is plain text or the cached result of a field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FieldRole {
    /// Plain text (not part of a field).
    #[default]
    None,
    /// A hyperlink — `url` from the `HYPERLINK` field instruction.
    Hyperlink {
        /// The link target.
        url: String,
    },
    /// A simple non-hyperlink field whose instruction is preserved.
    Simple {
        /// Normalized field instruction such as `PAGE`, `FILENAME \p`, or `REF Figure1`.
        instruction: String,
    },
    /// Any other field (PAGE, REF, …) whose result text is kept verbatim.
    Other,
}

/// Paragraph alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    /// Left-aligned (the default).
    #[default]
    Left,
    /// Centered.
    Center,
    /// Right-aligned.
    Right,
    /// Justified.
    Justify,
}

/// A table: rows of cells.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    /// The table's rows, top to bottom.
    pub rows: Vec<Row>,
    /// Number of leading header rows (repeated `sprmTTableHeader` rows).
    pub header_rows: usize,
    /// Column widths as fractions of the table width; empty = even split.
    pub col_widths_pct: Vec<f32>,
}

impl Table {
    /// `true` if every cell holds exactly one single-line paragraph and no cell
    /// spans more than one row or column — i.e. a clean GFM pipe table is lossless.
    pub fn is_simple_grid(&self) -> bool {
        self.rows.iter().all(|row| {
            row.cells.iter().all(|c| {
                c.row_span <= 1
                    && c.col_span <= 1
                    && c.blocks.len() <= 1
                    && c.blocks.iter().all(|b| match b {
                        Block::Paragraph(p) => !p.text().contains('\n'),
                        _ => false,
                    })
            })
        })
    }
}

/// A table row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    /// The row's cells, left to right.
    pub cells: Vec<Cell>,
}

/// Vertical alignment of cell content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VCell {
    /// Top (the default).
    #[default]
    Top,
    /// Vertically centered.
    Center,
    /// Bottom.
    Bottom,
}

/// A table cell — may hold block content and span rows/columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// Block-level content of the cell.
    pub blocks: Vec<Block>,
    /// Number of rows this cell spans (1 = no vertical merge).
    pub row_span: u16,
    /// Number of columns this cell spans (1 = no horizontal merge).
    pub col_span: u16,
    /// Whether this is a header cell (`<th>`).
    pub is_header: bool,
    /// Cell background shading, if any.
    pub shading: Option<Color>,
    /// Vertical alignment of the cell content.
    pub valign: VCell,
    /// Cell width as a fraction of the table width, if set (`None` = auto).
    pub width_pct: Option<f32>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            blocks: Vec::new(),
            row_span: 1,
            col_span: 1,
            is_header: false,
            shading: None,
            valign: VCell::Top,
            width_pct: None,
        }
    }
}

impl Cell {
    /// The cell's text, paragraphs joined by newlines.
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// An embedded image. `bytes`/`mime` are present only when the picture was
/// extracted; otherwise the node is a placeholder.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Image {
    /// Alt text, if any.
    pub alt: Option<String>,
    /// Raw image bytes, when extracted.
    pub bytes: Option<Vec<u8>>,
    /// MIME type of `bytes` (e.g. `image/png`), when known.
    pub mime: Option<String>,
    /// Intrinsic width in pixels, parsed from the image header, when known.
    pub width_px: Option<u32>,
    /// Intrinsic height in pixels, parsed from the image header, when known.
    pub height_px: Option<u32>,
}

/// Supported chart layouts for authored `.docx` output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChartKind {
    /// A clustered horizontal bar chart.
    #[default]
    Bar,
    /// A clustered 3-D horizontal bar chart.
    Bar3D,
    /// A clustered vertical column chart.
    Column,
    /// A clustered 3-D vertical column chart.
    Column3D,
    /// A line chart with category labels on the horizontal axis.
    Line,
    /// A 3-D line chart with category labels on the horizontal axis.
    Line3D,
    /// An area chart with category labels on the horizontal axis.
    Area,
    /// A 3-D area chart with category labels on the horizontal axis.
    Area3D,
    /// A radar chart with category labels around a radial axis.
    Radar,
    /// A scatter chart with numeric horizontal and vertical values.
    Scatter,
    /// A bubble chart with numeric horizontal values, vertical values, and sizes.
    Bubble,
    /// A pie chart using the first series as slice values.
    Pie,
    /// A 3-D pie chart using the first series as slice values.
    Pie3D,
    /// A pie-of-pie chart using the first series as slice values.
    PieOfPie,
    /// A bar-of-pie chart using the first series as slice values.
    BarOfPie,
    /// A doughnut chart using the first series as slice values.
    Doughnut,
    /// A surface chart using category columns and series rows as a value grid.
    Surface,
    /// A 3-D surface chart using category columns and series rows as a value grid.
    Surface3D,
    /// A stock chart using date/category labels and open/high/low/close-style series.
    Stock,
}

/// Supported shape styles for authored 3-D bar/column charts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChartShape {
    /// Rectangular 3-D boxes.
    #[default]
    Box,
    /// Cylindrical 3-D bars or columns.
    Cylinder,
    /// Cone-shaped 3-D bars or columns.
    Cone,
    /// Cone-shaped 3-D bars or columns scaled to the maximum value.
    ConeToMax,
    /// Pyramid-shaped 3-D bars or columns.
    Pyramid,
    /// Pyramid-shaped 3-D bars or columns scaled to the maximum value.
    PyramidToMax,
}

/// One named chart series with literal numeric values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChartSeries {
    /// Series display name.
    pub name: String,
    /// Values aligned with [`Chart::categories`].
    pub values: Vec<f64>,
    /// Optional bubble sizes aligned with [`ChartSeries::values`].
    pub bubble_sizes: Vec<f64>,
}

/// A block-level chart with literal category and numeric caches.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chart {
    /// Chart layout.
    pub kind: ChartKind,
    /// Optional display title.
    pub title: Option<String>,
    /// Category labels shared by all series.
    pub categories: Vec<String>,
    /// Named numeric series.
    pub series: Vec<ChartSeries>,
    /// Drawing width in pixels, interpreted at 96 dpi.
    pub width_px: Option<u32>,
    /// Drawing height in pixels, interpreted at 96 dpi.
    pub height_px: Option<u32>,
    /// Alternate text for the chart drawing.
    pub alt: Option<String>,
    /// Render surface-family charts as wireframes instead of filled surfaces.
    pub wireframe: bool,
    /// Shape style for 3-D bar and 3-D column charts.
    pub shape: ChartShape,
}

/// Page geometry, in points. Default is A4 portrait with 1-inch margins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSetup {
    /// Page width.
    pub width_pt: f32,
    /// Page height.
    pub height_pt: f32,
    /// Uniform margin fallback (used for any side without an explicit override).
    pub margin_pt: f32,
    /// Left margin override (points); falls back to `margin_pt`. Per-side overrides
    /// let asymmetric layouts (a wide left sidebar, a binding gutter) render with
    /// the correct content box instead of a forced-uniform margin.
    pub margin_left_pt: Option<f32>,
    /// Right margin override (points); falls back to `margin_pt`.
    pub margin_right_pt: Option<f32>,
    /// Top margin override (points); falls back to `margin_pt`.
    pub margin_top_pt: Option<f32>,
    /// Bottom margin override (points); falls back to `margin_pt`.
    pub margin_bottom_pt: Option<f32>,
    /// Landscape orientation (swaps width/height semantics on emit).
    pub landscape: bool,
}

impl PageSetup {
    /// Left margin (override or uniform fallback).
    pub fn left(&self) -> f32 {
        self.margin_left_pt.unwrap_or(self.margin_pt)
    }
    /// Right margin (override or uniform fallback).
    pub fn right(&self) -> f32 {
        self.margin_right_pt.unwrap_or(self.margin_pt)
    }
    /// Top margin (override or uniform fallback).
    pub fn top(&self) -> f32 {
        self.margin_top_pt.unwrap_or(self.margin_pt)
    }
    /// Bottom margin (override or uniform fallback).
    pub fn bottom(&self) -> f32 {
        self.margin_bottom_pt.unwrap_or(self.margin_pt)
    }
}

impl Default for PageSetup {
    fn default() -> Self {
        // A4 = 210×297mm = 595.3×841.9pt; 1in = 72pt margins.
        PageSetup {
            width_pt: 595.3,
            height_pt: 841.9,
            margin_pt: 72.0,
            margin_left_pt: None,
            margin_right_pt: None,
            margin_top_pt: None,
            margin_bottom_pt: None,
            landscape: false,
        }
    }
}

/// Section-level layout recovered from or generated into `.docx` section
/// properties.
///
/// In WordprocessingML a section property block closes the section that came
/// before it. `Block::SectionBreak(setup)` follows that convention: the stored
/// setup describes the section ending at that block, while `DocModel::setup`
/// describes the final section.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionSetup {
    /// Page geometry.
    pub page: PageSetup,
    /// Running header content (empty = none).
    pub header: Vec<Block>,
    /// First-page running header content (empty = none or default applies).
    pub first_header: Vec<Block>,
    /// Even-page running header content (empty = none or default applies).
    pub even_header: Vec<Block>,
    /// Running footer content (empty = none).
    pub footer: Vec<Block>,
    /// First-page running footer content (empty = none or default applies).
    pub first_footer: Vec<Block>,
    /// Even-page running footer content (empty = none or default applies).
    pub even_footer: Vec<Block>,
    /// Emit a centered page number (`PAGE` field) in the footer.
    pub page_numbers: bool,
}

/// Document-level layout + metadata, for authoring and rendering. All fields are
/// optional/default so existing read paths are unaffected.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocSetup {
    /// Page geometry.
    pub page: PageSetup,
    /// Paragraph style definitions for generated `.docx` output.
    pub styles: Vec<ParagraphStyle>,
    /// Running header content (empty = none).
    pub header: Vec<Block>,
    /// First-page running header content (empty = none or default applies).
    pub first_header: Vec<Block>,
    /// Even-page running header content (empty = none or default applies).
    pub even_header: Vec<Block>,
    /// Running footer content (empty = none).
    pub footer: Vec<Block>,
    /// First-page running footer content (empty = none or default applies).
    pub first_footer: Vec<Block>,
    /// Even-page running footer content (empty = none or default applies).
    pub even_footer: Vec<Block>,
    /// Emit a centered page number (`PAGE` field) in the footer.
    pub page_numbers: bool,
    /// Document title metadata.
    pub title: Option<String>,
    /// Document author metadata.
    pub creator: Option<String>,
}

impl From<&DocSetup> for SectionSetup {
    fn from(setup: &DocSetup) -> Self {
        SectionSetup {
            page: setup.page,
            header: setup.header.clone(),
            first_header: setup.first_header.clone(),
            even_header: setup.even_header.clone(),
            footer: setup.footer.clone(),
            first_footer: setup.first_footer.clone(),
            even_footer: setup.even_footer.clone(),
            page_numbers: setup.page_numbers,
        }
    }
}
