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

/// A whole `.doc` document as an ordered list of block-level nodes plus
/// document-level metadata.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocModel {
    /// Block-level content, in reading order.
    pub blocks: Vec<Block>,
    /// Document-level metadata (codepage, language, counts).
    pub meta: DocMeta,
    /// Document-level layout for authoring/rendering (page size, header/footer,
    /// metadata). Defaults to A4 portrait with no running header/footer.
    pub setup: DocSetup,
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
    /// Small caps.
    pub small_caps: bool,
}

/// Whether a run is plain text or the result of a field (only the field *result*
/// reaches the model; the instruction is parsed away).
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

/// Page geometry, in points. Default is A4 portrait with 1-inch margins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSetup {
    /// Page width.
    pub width_pt: f32,
    /// Page height.
    pub height_pt: f32,
    /// Uniform margin (all four sides).
    pub margin_pt: f32,
    /// Landscape orientation (swaps width/height semantics on emit).
    pub landscape: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        // A4 = 210×297mm = 595.3×841.9pt; 1in = 72pt margins.
        PageSetup {
            width_pt: 595.3,
            height_pt: 841.9,
            margin_pt: 72.0,
            landscape: false,
        }
    }
}

/// Document-level layout + metadata, for authoring and rendering. All fields are
/// optional/default so existing read paths are unaffected.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocSetup {
    /// Page geometry.
    pub page: PageSetup,
    /// Running header content (empty = none).
    pub header: Vec<Block>,
    /// Running footer content (a `PAGE` field run renders page numbers).
    pub footer: Vec<Block>,
    /// Document title metadata.
    pub title: Option<String>,
    /// Document author metadata.
    pub creator: Option<String>,
}
