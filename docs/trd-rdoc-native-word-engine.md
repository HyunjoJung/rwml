# TRD - rdoc native Word engine

Technical design for the long-term direction described in
[`prd-rdoc-native-word-engine.md`](prd-rdoc-native-word-engine.md).

**Status:** Draft, active implementation baseline
**Updated:** 2026-06-29
**Scope:** architecture and implementation direction for reader depth,
preservation editing, authoring, rendering, diagnostics, and validation.

## 1. Current Architecture

rdoc already has the right core split:

- `.doc` parser: OLE/CFB, FIB, CLX piece table, CHPX/PAPX, STSH, lists, images;
- `.docx` parser: OPC/ZIP, WordprocessingML body, styles, numbering, rels,
  media, headers/footers, notes, text boxes;
- shared model: `DocModel`, `Block`, `Paragraph`, `Run`, `Table`, `Image`,
  `DocSetup`;
- exporters: Markdown and HTML;
- writer: `.docx` generator from `DocModel`;
- preservation editor: retained OPC package plus element-tree operations for body
  text replacement and PNG/JPEG image insertion and replacement;
- renderer: feature-gated PDF output through the model.

The main technical issue is not missing ambition. It is that the next layers must
not overload `DocModel`. A semantic model, a preservation package, and a layout
tree are different representations.

## 2. Target Architecture

The target architecture has six layers.

```text
raw bytes
  -> format parser (.doc / .docx)
  -> retained source package or binary streams
  -> semantic model + feature inventory
  -> edit surface / authoring surface
  -> render layout tree
  -> export targets (text, md, html, docx, pdf, diagnostics)
```

### 2.1 Format parsers

Format parsers decode source structures and keep enough raw context to build a
semantic view and a diagnostics report.

`.doc` parser responsibilities:

- OLE stream discovery and validation;
- FIB and table stream navigation;
- piece table decoding;
- subdocument boundaries;
- CHPX/PAPX/STSH/list/image extraction;
- field, header/footer, footnote/endnote, textbox, and shape detection;
- typed errors for encrypted, old, malformed, or unsupported files.

`.docx` parser responsibilities:

- OPC package validation;
- XML part discovery;
- relationship and content-type resolution;
- WordprocessingML semantic extraction;
- comment/revision/field/shape/content-control detection;
- raw package retention for editing.

### 2.2 Retained source representation

For `.docx`, the retained package is the source of truth for preservation edits.
The current `opc` and `xmltree` direction should continue:

- package parts retained as raw bytes until changed;
- content types parsed and regenerated only when necessary;
- relationship graph preserved;
- XML promoted to an editable tree lazily;
- part size, node count, depth, attribute, and total archive budgets enforced.

For `.doc`, retention is read-only at first. The parser can keep streams needed
for semantic extraction, but public editing should convert to `.docx` rather than
attempt in-place binary write-back.

### 2.3 Semantic model

`DocModel` should stay the cross-format read/render/authoring view, but it needs
extension points.

Current core:

- `Block::Paragraph`;
- `Block::Table`;
- `Block::Image`;
- `Block::Chart`;
- `Block::PageBreak`;
- `Block::SectionBreak`;
- `Run` with direct character properties;
- `FieldRole` for hyperlinks, simple field instructions, and field result text;
- `DocSetup` for page/header/footer basics and generated core metadata
  (`title`, `subject`, `creator`, `description`, `keywords`, `category`,
  `contentStatus`, `lastModifiedBy`, `created`, `modified`, `lastPrinted`,
  `revision`, and `version`).

Target additions:

```rust
pub struct DocModel {
    pub blocks: Vec<Block>,
    pub regions: Vec<SourceRegion>,
    pub meta: DocMeta,
    pub setup: DocSetup,
    pub features: FeatureInventory,
    pub annotations: AnnotationStore,
}
```

`FeatureInventory` reports what the document contains:

- comments;
- footnotes/endnotes;
- text boxes;
- tracked insertions/deletions/moves;
- fields by kind;
- hyperlinks;
- content controls;
- nested tables;
- floating shapes;
- OLE objects;
- charts;
- unsupported image formats, including structured WMF/EMF/EMZ/WMZ metadata
  for package path, format, stored byte size, compression flag, and raw-header
  dimensions when recoverable from raw or gzip-wrapped headers;
- unsupported scripts/layout directions;
- malformed-but-recovered structures.

`AnnotationStore` can hold side tables without forcing every annotation into the
linear block stream:

```rust
pub struct AnnotationStore {
    pub comments: Vec<Comment>,
    pub revisions: Vec<Revision>,
    pub fields: Vec<Field>,
    pub anchors: Vec<Anchor>,
}
```

This avoids prematurely inventing a perfect inline representation while still
making features visible to callers.

### 2.4 Edit surface

Preservation edits must operate on retained package state.

Short-term surfaces:

- `replace_body_text`;
- `add_image_png`;
- `replace_image_png`;
- `add_image_jpeg`;
- `replace_image_jpeg`;
- `add_image_gif`;
- `replace_image_gif`;
- `add_image_bmp`;
- `replace_image_bmp`;
- `add_image_tiff`;
- `replace_image_tiff`;
- `add_image_webp`;
- `replace_image_webp`;
- `edit_capability`;
- `replace_text_in_part`;
- `set_core_property`;
- `replace_header_footer_text`;
- `add_footnote_on_text`;
- `add_endnote_on_text`;
- `replace_note_text`;
- `set_field_result`;
- `fill_content_control_by_tag`;
- `fill_content_controls_by_tag`;
- `fill_template_fields`;
- `accept_all_revisions` for tracked body revisions in `word/document.xml`;
- `reject_all_revisions` for tracked body revisions in `word/document.xml`;
- `set_hyperlink_target`;
- `set_comment_text`;
- `add_comment_on_text`;
- `set_table_cell_text` for accepted-current top-level body-table cells using
  `gridSpan`-aware logical columns and `vMerge`-aware logical rows.

Each edit must declare:

- touched parts;
- relationship changes;
- content-type changes;
- whether it preserves unmodeled body content;
- rollback behavior on failure.

Use a transaction object for multi-part edits:

```rust
struct PackageTransaction {
    original: PackageSnapshot,
    writes: Vec<PartWrite>,
    rel_edits: Vec<RelEdit>,
    content_type_edits: Vec<ContentTypeEdit>,
}
```

The transaction commits only after all validation passes.

### 2.5 Authoring surface

Authoring should be model-first and ergonomic. It should not pretend to preserve
a foreign package.

Target API direction:

```rust
let doc = DocBuilder::new()
    .margins_pt(54.0)
    .header_runs([RunBuilder::new("Report").bold().build()])
    .footer_runs([RunBuilder::new("Page ").italic().build()])
    .page_numbers()
    .paragraph_style(
        ParagraphStyleBuilder::new("RiskCallout", "Risk callout")
            .based_on("Normal")
            .shading(Color::rgb(0xFE, 0xF2, 0xF2))
            .run_bold()
            .run_color(Color::rgb(0xC0, 0x00, 0x00)),
    )
    .heading(1, "Report")
    .paragraph("Summary")
    .rich_paragraph(ParagraphBuilder::new().runs([
        RunBuilder::new("At risk")
            .comment(CommentBuilder::new("Needs owner").author("Reviewer"))
            .build(),
        RunBuilder::new(" - ").build(),
        RunBuilder::new("Guide")
            .hyperlink("https://example.com/guide")
            .underline()
            .build(),
    ]).style("RiskCallout"))
    .numbered_list(["Collect", "Publish"])
    .bullet_list_level(1, ["Assign owner"])
    .field("FILENAME \\p", "report.docx")
    .hyperlink("Project", "https://example.com/")
    .rich_table(
        TableBuilder::new()
            .header_rows(1)
            .col_widths_pct([0.7, 0.3])
            .row([
                CellBuilder::text("Metric").shading(Color::rgb(0x1F, 0x38, 0x64)),
                CellBuilder::text("Value").shading(Color::rgb(0x1F, 0x38, 0x64)),
            ])
            .row([CellBuilder::text("Openable"), CellBuilder::text("Yes")]),
    )
    .section_break()
    .clear_header()
    .page_size_pt(792.0, 612.0)
    .landscape()
    .header_runs([RunBuilder::new("Follow-up").bold().build()])
    .heading(2, "Follow-up")
    .build();

let bytes = rdoc::try_write_docx(&doc)?;
```

`DocBuilder` is now a thin wrapper over `DocModel` for plain paragraphs,
layout-aware paragraphs via `ParagraphBuilder`, headings, styled runs via
`RunBuilder`, named paragraph style definitions via `ParagraphStyleBuilder`,
simple text tables, width-aware, aligned, fixed-layout, indented, uniform/per-side border-width, border-style, and border-color rich tables, explicit
cell margins, and typed nested cell blocks via `TableBuilder`/`CellBuilder`,
list paragraphs with explicit levels, simple fields
with cached results, run-anchored
comments via `CommentBuilder` including reply parent ids, commentsExtended
metadata, and authored tab/newline markers,
tracked insertion/deletion runs through
`RevisionBuilder`, run-level content controls through `ContentControlBuilder`,
hyperlinks, image blocks with alt text and explicit pixel sizing via
`ImageBuilder`, bar/stacked bar/100% stacked bar/3-D bar/stacked 3-D bar/100% stacked 3-D bar/column/stacked column/100% stacked column/3-D column/stacked 3-D column/100% stacked 3-D column/line/markerless line/smooth line/stacked line/100% stacked line/3-D line/area/stacked area/100% stacked area/3-D area/stacked 3-D area/100% stacked 3-D area/radar/radar-with-markers/filled radar/scatter/line-only scatter/smooth scatter/smooth markerless scatter/marker-only scatter/bubble/3-D bubble/pie/exploded pie/3-D pie/exploded 3-D pie/doughnut/exploded doughnut/surface/3-D surface/high-low-close stock/stock/pie-of-pie/bar-of-pie
charts with embedded workbook-backed source data through `ChartBuilder`,
3-D bar/column-family shape styling through `ChartBuilder::shape(ChartShape::...)`,
surface-family wireframe styling through `ChartBuilder::wireframe()`,
page size/orientation/margins/columns/document grids/text direction/title pages, page-number restarts/formats, explicit Word document ids, web-extension task pane package shells,
explicit page breaks and next/even/odd section breaks, default/first/even headers and footers, page numbers, and metadata. It does not
replace the lower-level structs for custom sections or complex nested layouts.

### 2.6 Render layer

The renderer should eventually stop consuming only the simple `DocModel` directly.
It needs a layout-oriented IR that can represent pages, flow regions, anchors,
and diagnostics.

Pipeline:

```text
DocModel + FeatureInventory
  -> RenderInput
  -> LayoutTree
  -> PageList
  -> PdfBackend / future SvgBackend / future raster backend
```

`RenderInput` should include:

- page setup;
- resolved styles;
- blocks;
- annotations relevant to layout;
- image resources;
- font resources;
- field display policy;
- unsupported placeholders.

`LayoutTree` should make pagination inspectable for tests:

- pages;
- flow boxes;
- table fragments;
- line boxes;
- positioned objects;
- link annotations;
- text spans and glyph/source ranges.

## 3. Feature Diagnostics

Diagnostics are the bridge between "not supported yet" and "silently wrong".

Add a public report:

```rust
pub struct DocumentReport {
    pub format: DocumentFormat,
    pub stats: Stats,
    pub core_properties: CoreProperties,
    pub custom_properties: BTreeMap<String, String>,
    pub edit: EditCapability,
    pub edited_parts: Vec<String>,
    pub features: FeatureInventory,
    pub warnings: Vec<DocumentWarning>,
}
```

Example warnings:

- `UnsupportedFieldEvaluation { kind: "CUSTOM" }`;
- `TrackedChangesOriginalViewUnavailable`;
- `FloatingShapePlaceholderOnly`;
- `UnsupportedMetafileImages { count }`, with details in
  `FeatureInventory::metafiles`;
- `EncryptedDocumentRejected`;
- `MalformedXmlRecovered { part }`;
- `PackageReadOnly { reason }`.

Expose:

```rust
impl Document {
    pub fn report(&self) -> DocumentReport;
    pub fn edit_capability(&self) -> EditCapability;
}
```

Diagnostics should be stable enough for downstream tests and CLI output.

## 4. Reader Deepening Plan

### 4.1 `.docx` comments

Parts:

- `word/comments.xml`;
- `word/_rels/comments.xml.rels`;
- comment range markers in `document.xml`;
- comment references in runs.

Implementation:

- parse comments into `AnnotationStore.comments`;
- attach anchors by comment id and range start/end;
- treat `w:tab`, `w:br`, `w:cr`, `w:noBreakHyphen`, and `w:softHyphen` as
  visible inline text markers in comment bodies and anchor ranges;
- keep comment bodies and anchor text on the accepted/current view: direct,
  inserted, and moved-to text is visible, while deleted and moved-from old-only
  text is omitted;
- preserve raw comments part on save;
- expose counts and orphan markers through diagnostics;
- later add `add_comment`.

Acceptance:

- comments are counted;
- comment text is extractable;
- anchors are stable and follow the accepted/current revision policy;
- no-op save preserves the comments part.

### 4.2 `.docx` tracked changes

Structures:

- `w:ins`;
- `w:del`;
- `w:moveFrom`;
- `w:moveTo`;
- property changes.

Implementation:

- add read policies: accepted view, original view, annotated view;
- default extraction remains accepted/current view, descending inline and
  block-level `w:ins`/`w:moveTo` current-content wrappers while omitting
  `w:del`/`w:moveFrom` old-content wrappers;
- diagnostics report revision counts;
- semantic model can expose revisions in `AnnotationStore`.

Acceptance:

- heavy tracked-change docs no longer look unexplained in parity reports;
- callers can choose at least accepted vs annotated view.

### 4.3 Fields

Structures:

- simple fields;
- complex fields with begin/separate/end runs;
- cached result text;
- instruction text.

Implementation:

- preserve simple field instructions on `FieldRole::Simple` and expose richer
  extracted fields through `Document::fields()`;
- keep `.docx` `Document::fields()` on the accepted/current revision view and
  single-branch `mc:AlternateContent` view: include direct, inserted, and
  moved-to current fields while omitting deleted, moved-from old, and redundant
  Choice/Fallback field serializations;
- for legacy `.doc`, derive `Document::fields()` and `Document::report()` field
  counts from field-marked model runs, coalescing adjacent formatted result runs
  that belong to the same normalized instruction;
- keep cached result text in visible runs and in `Field::result`, preserving
  simple inline tabs, line breaks, and no-break/soft hyphens for simple and common
  complex body fields;
- parse field kind and raw instruction;
- compute only low-risk fields at first (`PAGE` in renderer context and trusted
  structural/source-rendered reader current-page context, field-code
  `HYPERLINK` as link annotations with target/anchor, tooltip/frame, and
  documented `\m`/`\n` no-op switch tails, with malformed hyperlink syntax
  reporting `UnsupportedSwitch`, `FILENAME` with malformed switches reporting
  `UnsupportedSwitch`, `MERGEFIELD` with malformed merge-field names or switch tails reporting
  `UnsupportedSwitch`,
  metadata-backed document-info fields such as `AUTHOR`, `TITLE`,
  `SUBJECT`, `KEYWORDS`, `COMMENTS`, `LASTSAVEDBY`, core aliases such as
  `CREATOR`, `DESCRIPTION`, `KEYWORD`, and `LASTMODIFIEDBY`, and mapped `DOCPROPERTY`
  names from `docProps/core.xml`, `docProps/custom.xml`, or `docProps/app.xml`,
  including core `CATEGORY`/`CONTENTSTATUS`/`VERSION` fields,
  plus mapped `INFO` package-property subfields, mapped `DOCVARIABLE` names from
  `word/settings.xml`, timestamp-shaped custom `DOCPROPERTY` values with simple
  `\@` pictures, core timestamp-backed
  `CREATEDATE`/`SAVEDATE`/`PRINTDATE` with quoted or switch-delimited unquoted
  simple numeric and English month/weekday `\@` pictures, app-property-backed
  `NUMPAGES`/`NUMWORDS`/`NUMCHARS`/`EDITTIME`/`TEMPLATE` values and common
  scalar built-ins such as `Company`/`Manager`/`HyperlinkBase`/`DocSecurity`
  from `docProps/app.xml`, including direct scalar app-property field names
  such as `APPLICATION`, `APPVERSION`, `COMPANY`, `MANAGER`, `HYPERLINKBASE`,
  `DOCSECURITY`, and `LINKSUPTODATE`, `FILESIZE` from the opened `.docx`
  package byte length with raw byte output and rounded `\k` kilobyte/`\m`
  megabyte switches, direct `USERNAME`/`USERINITIALS`/`USERADDRESS` fields with
  explicit quoted literal overrides, and cached
  date/user/unmapped document-info fields
  render-supported display fields when their instruction syntax is valid;
  malformed document-info syntax reports `UnsupportedSwitch`);
- compute unambiguous `.docx` `REF` bookmark targets, including Word-generated
  hidden bookmark targets, multi-paragraph bookmark ranges, and simple inline
  tabs, line breaks, no-break/soft hyphens, and deterministic `REF \* Upper`/
  `REF \* Lower`/`REF \* Caps`/`REF \* FirstCap` text format switches plus
  source-order `REF \p` relative-position results, plus direct bookmark-name
  field computation when the bookmark exists with supported text-format switches
  and neutral `\h`, explicit numbered-paragraph `\n`/`\n \t`/`\r`/`\r \t`/`\w`/`\w \t`
  from single-branch source paragraphs,
  note-reference `\f`, sequence-separator `\d`, and source-order `\p`,
  plus `REF \n \p` relative suffixes, `REF \n \t` numeric-text suppression,
  `REF \r` relative-context numbered labels with `\r \p` relative suffixes and
  `\r \t` numeric-text suppression when the REF field paragraph also has an
  unambiguous numbering context,
  and `REF \w` full-context numbered labels with `\w \p` relative suffixes
  and `\w \t` numeric-text suppression,
  plus `REF \f` visible body footnote/endnote reference marks, body
  comment-reference markers, exact matching bookmarked comment ranges, and
  bookmarks wholly inside body comment ranges,
  counting prior generated REF note marks in source order plus common
  field-result number/text format switches, and text-neutral
  `REF \d "separator"` bookmark text while value-changing sequence/page
  separator cases preserve cached text until sequence/page separator semantics
  are modeled,
  as `Field::computed_result` and use that text in the read/render model
  instead of stale cached text for simple and common complex fields;
- compute bookmarked `.docx` `NOTEREF` and legacy `FTNREF` fields when the
  bookmark encloses a body `footnoteReference` or `endnoteReference`, with
  separate source-order footnote/endnote numbering, neutral `\h`,
  note-reference-style `\f`, source-order `\p` above/below results, and common
  field-result number/text format switches, while
  preserving cached text for missing note bookmarks, existing bookmarks without
  body note-reference marks, or unsupported switches;
- compute bare default `.docx` `TOC` heading ranges, standalone bookmark-scoped
  default `TOC \b`, explicit `TOC \o`
  heading-outline ranges including omitted all-level ranges and common `\o`/`\u` combinations with
  value-neutral `\h`/`\z` switches, text-preserving `\w`/`\x` switches
  normalized to plain text, and text-neutral `\n` no-page-number,
  `\p` entry/page separator, `\s` sequence-number page prefix, and `\d`
  sequence/page separator switches, deterministic TOC `\* Upper`/`\* Lower`/
  `\* Caps`/`\* FirstCap` field-result format switches,
  neutral TOC `\* MERGEFORMAT`/`\* MERGEFORMATINET`/`\* CHARFORMAT`,
  quoted or switch-delimited unquoted `TOC \t` custom-style entries,
  `TOC \f` entries from matching `TC "Text"` markers with optional `\f` type
  identifiers, `\l` levels, and common marker text-format tails, with supported
  `TC` marker fields themselves rendering as hidden output and unsupported `TC`
  marker syntax preserving cached text with `UnsupportedSwitch` diagnostics, `TOC \c` full-caption entries and `TOC \a`
  label/number-omitted caption-text entries from paragraphs containing
  matching `SEQ Identifier` fields, with simple or common complex dirty/stale
  `SEQ` caption numbers recomputed from source order,
  and standalone `TOC \u` fields over explicit paragraph outline levels, plus
  `TOC \b` bookmark-scoped variants when the bookmark range is recoverable, as
  `Field::computed_result`, including empty computed results for existing
  scopes with no matching entries, normalizing simple inline heading/caption
  tabs, line breaks, no-break/soft hyphens, and supported literal symbols, and
  use that text in the read/render model instead of stale cached text for simple
  and common complex fields;
- compute deterministic body paragraph- and character-style `.docx`
  `STYLEREF` fields by matching style id or quoted/switch-bounded unquoted
  style name, searching backward from the field and falling forward when no
  earlier match exists, exposing the result as `Field::computed_result`, and
  using that text in the read/render model for simple and common complex fields
  with neutral/general text format switches; compute source-order `\p`
  above/below results and deterministic
  numbered source paragraphs with `\n`, `\r`, `\w`, and numeric-text `\t`
  switches from the existing numbering context; keep
  cached text for page-aware/header-footer lookup and other unsupported
  `STYLEREF` variants, with malformed `REVNUM`/`STYLEREF` syntax reported as
  `UnsupportedSwitch`;
- compute `.docx` `SECTIONPAGES` fields for structurally bounded sections whose
  page count comes only from source hard breaks, enabled `pageBreakBefore`, and
  section structure without layout inference, exposing the result as `Field::computed_result` and using
  that text in the read/render model for simple and common complex fields with
  page-number and general field-result format switches; keep cached text for
  layout-derived section page counts;
- compute deterministic `.docx` `EQ \f(n,d)` simple fraction fields with comma
  or semicolon separators, literal/spaced/quoted operands, documented escaped
  comma/open-parenthesis/backslash operand characters, and parenthesized nested
  simple `EQ \f`/`\r` operands as plain `n/d` text, exposing the value as
  `Field::computed_result` and using that text in the read/render model for
  simple and common complex fields;
- compute deterministic `.docx` `EQ \r(radicand)` and
  `EQ \r(degree,radicand)` radical fields as plain root text, exposing the value
  as `Field::computed_result` and using that text in the read/render model for
  simple and common complex fields; keep cached text for broader equation layout
  and nested equation constructs;
- compute deterministic default/custom `.docx` `EQ \b(element)` bracket fields
  with documented `\lc`, `\rc`, or `\bc` options as bracketed plain text and
  `EQ \x(element)` boxed operand fields, including documented border-side
  options, as enclosed operand plain text; keep cached text for broader equation
  layout;
- compute deterministic `.docx` `EQ \l(...)` list fields as comma-joined
  operand plain text using the same literal, quoted, escaped, and nested simple
  operand subset as fractions/radicals/brackets;
- compute deterministic `.docx` `EQ \a(...)` array fields with documented
  alignment, column-count, and spacing options as tab-separated columns and
  newline-separated rows over supported row-major operands; keep cached text for
  broader equation layout;
- compute deterministic `.docx` `EQ \s` script fields with documented `\up n`
  and `\do n` options as `^`/`_` marker plain text, preserving non-empty
  `\ai n(...)`/`\di n(...)` operands as plain text, hiding empty layout-only
  controls, and preserving cached text for broader stacked script layout;
- compute deterministic `.docx` `EQ \i(...)` integral fields, including
  documented `\su`, `\pr`, `\in`, `\fc\c`, and `\vc\c` options, as symbol plus
  `_`/`^` limit marker plain text over supported three-operand forms;
- compute deterministic `.docx` `EQ \o(...)` overstrike fields, including
  documented `\al`, `\ac`, and `\ar` alignment options, as source-order overlay
  operand plain text over the supported operand subset;
- compute deterministic `.docx` `EQ \d` displacement controls with documented
  `\fo n`, `\ba n`, and `\li` options as supported operand text, or empty
  output for empty controls, in `Field::computed_result`, without applying
  visual offsets or underlines;
- compute deterministic `.docx` `ADVANCE` fields with validated point movement
  switches (`\d`, `\u`, `\l`, `\r`, `\x`, `\y`) as hidden output in
  `Field::computed_result`, accepting field-result format switches without applying
  the layout offsets to surrounding text;
- compute deterministic `.docx` `SYMBOL` fields for decimal/hex/default ANSI
  codepoints, Unicode `\u`, neutral `\h`, separated font `\f` switches with
  quoted or switch-delimited unquoted operands, compact single-token font `\f`
  switches, quoted or unquoted separated/compact size `\s` switches, and common Symbol/Wingdings
  font mappings including Symbol `0xB7` bullet, plus field-result format switches, exposing the inserted character as `Field::computed_result` and
  using that text in the read/render model for simple and common complex fields;
  keep cached text for Shift-JIS and broader font-specific symbol mappings;
- classify invalid display/layout and action/automation field syntax as
  `UnsupportedSwitch`, while valid broader forms that need unsupported layout,
  equation, symbol, action, macro, printer, or PostScript semantics keep cached
  text with `NoComputedResult`;
- compute body `PAGE` fields from trusted leading structural or source-rendered
  current-page context, including accepted/current wrappers, single-branch
  `mc:AlternateContent` page markers, section `w:pgNumType` displayed
  page-number restarts/styles, deterministic display-only explicit `w:start`
  labels for immediate section-start `PAGE` fields after visible intro text,
  deterministic page-number format switches, and common field-result format
  switches; preserve cached text for
  visible-content manual-break and broader layout-derived current-page cases
  with `NoComputedResult` diagnostics;
- classify `PAGEREF` as a named field kind, compute page numbers only when the
  bookmark page is structural from leading explicit page breaks before any
  visible body content, enabled paragraph `w:pageBreakBefore`, explicit or
  default `nextPage`, and explicit `evenPage`/`oddPage` section starts in
  leading or trusted rendered context, including deterministic display-only
  `w:pgNumType w:start` page-number restart labels and trusted supported
  `w:pgNumType w:fmt` styles
  (`decimal`, `decimalZero`, `numberInDash`, `decimalFullWidth`,
  `decimalHalfWidth`, `decimalFullWidth2`, `decimalEnclosedCircle`,
  `decimalEnclosedFullstop`, `decimalEnclosedParen`, `ganada`, `chosung`,
  `koreanDigital`, `koreanCounting`, `koreanLegal`, `koreanDigital2`,
  lower/upper letter, lower/upper roman, ordinal/cardinal text) on those section starts and
  single-section final `body/sectPr` page-number defaults, source-persisted
  `w:lastRenderedPageBreak` markers scanned with the same single-branch
  `mc:AlternateContent` policy as flat text, or
  explicit hard breaks after a trusted
  leading/rendered page context,
  apply deterministic `\* Arabic`, `\* alphabetic`/`\* ALPHABETIC`,
  `\* roman`/`\* ROMAN`, `\* Ordinal`, `\* CardText`, `\* OrdText`, and
  `\* Hex`, integer-valued `\* DollarText`, and page-number-only `\* ArabicDash`
  number-format switches plus common field-result format switches, compute `\p`
  relative results (`above`, `below`, or `on page N`) when trusted leading
  structural page context, source page markers, deterministic display-only
  restart target/order context, or a paragraph-end section break after the
  target provide deterministic target/field ordering, and preserve cached
  page-reference text for remaining layout-dependent cases while
  leaving full layout-derived bookmark-to-page computation unsupported;
- report unsupported evaluation only for unknown fields beyond named
  document-info/date/stat display fields, dynamic/control fields beyond
  deterministic literal arithmetic/comparison/scalar-function formula fields with
  finite decimal/scientific numeric literals, finite literal exponentiation, and comma or semicolon function argument separators, simple non-spanning table
  references over existing plain numeric cells and source-order prior computed
  formula-only cells, including direct A1/RnCn cell references plus aggregate
  references over positional `LEFT`/`RIGHT`/`ABOVE`/`BELOW`, current `R`/`C`, A1
  cell/range, and RnCn cell/range references, skipping absent cells in ragged rows and including
  nested aggregate calls inside literal formula expressions, simple separated `\#`
  numeric pictures with quoted or switch-delimited unquoted operands plus compact single-token `\#` numeric pictures,
  literal affixes, single-section leading `+`/`-` sign-control items, `x`
  digit-drop/rounding positions, and sectioned
  positive/negative/zero pictures, plus neutral `\* MERGEFORMAT`/`\* MERGEFORMATINET`/`\* CHARFORMAT`
  formula tails, formula result text-format tails such as `\* Upper`/`\* Lower`/`\* Caps`/`\* FirstCap`,
  and literal/table formula general-number tails such as `\* DollarText` cents output and integer-valued `\* Hex`/`\* OrdText`,
  malformed formula switch syntax reporting `UnsupportedSwitch`,
  and literal `DEFINED(expr)` checks for parser-local literal expressions and source-order prior bookmark names,
  guarded scalar/table numeric `IF` branch selection skips unsupported or span-unsafe unselected branches, literal
  quoted/unquoted literal `QUOTE`, literal `IF` finite numeric/text comparisons, literal `COMPARE`
  finite numeric/text results, explicit-default `FILLIN`/`ASK` prompt fields,
  valid no-default prompt fields cached with `NoComputedResult`, and literal quoted or unquoted
  `SET` bookmark assignments, including multi-token unquoted payloads, with field-result format switches feeding later
  plain `REF`/direct bookmark references and source-order bookmark-backed
  `IF`/`COMPARE`/`NEXTIF`/`SKIPIF` comparisons, including numeric comparison
  for finite numeric bookmark values, plus literal `NEXT` and literal or source-order
  bookmark-backed `NEXTIF`/`SKIPIF` merge-control fields with field-result format
  switches that render as hidden output without running a mail merge,
  inserted-content fields, mail-merge helper
  fields, reference/index fields beyond hidden simple literal `RD`/`TA`/`XE` markers,
  numbering/list fields, document-structure
  fields beyond computed `REVNUM`/`SECTION`, structurally bounded `SECTIONPAGES`, and
  deterministic body paragraph- and character-style `STYLEREF`,
  hidden validated `ADVANCE`, deterministic literal simple `EQ` fractions/radicals/lists/arrays/scripts/integrals/overstrikes, default and custom brackets, boxed operands including nested simple operands, operand-preserving or hidden empty displacement controls, and `SYMBOL`,
  invalid display/layout syntax, remaining valid broader display/layout fields,
  invalid action/automation syntax, action/automation fields beyond deterministic quoted/unquoted formatted display text and hidden validated `PRINT` direct/group forms,
  compatibility/private fields, barcode fields, and legacy form fields beyond
  deterministic `w:ffData` checkbox checked/default states, dropdown
  result/default selections, explicit non-empty text-input current values, and
  empty-current text-input default results;
  named dynamic/control fields beyond deterministic literal arithmetic/comparison/scalar-function formula
  fields with comma or semicolon function argument separators, simple non-spanning
  table direct A1/RnCn cell references plus aggregate references over existing
  plain numeric positional `LEFT`/`RIGHT`/`ABOVE`/`BELOW`, current `R`/`C`, A1
  cell/range, and RnCn cell/range references, skipping absent cells in ragged rows and including
  nested aggregate calls inside literal formula expressions, simple separated `\#`
  numeric pictures with quoted or switch-delimited unquoted operands plus compact single-token `\#`
  numeric pictures, literal affixes, single-section leading `+`/`-` sign-control items, `x`
  digit-drop/rounding positions, and sectioned positive/negative/zero pictures,
  plus neutral `\* MERGEFORMAT`/`\* MERGEFORMATINET`/`\* CHARFORMAT` formula tails,
  formula result text-format tails such as `\* Upper`/`\* Lower`/`\* Caps`/`\* FirstCap`, and
  literal/table formula general-number tails such as `\* DollarText` cents output and integer-valued `\* Hex`/`\* OrdText`,
  malformed formula switch syntax reporting `UnsupportedSwitch`,
  quoted/unquoted literal `QUOTE`, literal `IF` finite numeric/text comparisons, literal
  `COMPARE` finite numeric/text results, explicit-default `FILLIN`/`ASK`
  prompt fields, valid no-default prompt fields cached with
  `NoComputedResult`, and
  literal quoted or unquoted `SET` bookmark assignments, including multi-token unquoted payloads, with field-result
  format switches feeding later plain `REF`/direct bookmark references and
  source-order bookmark-backed `IF`/`COMPARE`/`NEXTIF`/`SKIPIF` comparisons,
  including numeric comparison for finite numeric bookmark values, with malformed
  `SET` syntax reporting `UnsupportedSwitch`, plus literal `NEXT` and literal
  or source-order bookmark-backed `NEXTIF`/`SKIPIF` merge-control fields with field-result format
  switches that render as hidden output without running a mail merge,
  inserted-content, mail-merge helper, reference/index, numbering/list,
  document-structure fields beyond computed `REVNUM`/`SECTION`, structurally bounded
  `SECTIONPAGES`, and deterministic body paragraph- and character-style
  `STYLEREF`, hidden validated `ADVANCE`, deterministic simple `EQ` fractions/radicals/lists/arrays/scripts/integrals/overstrikes, default and custom brackets, boxed operands including nested simple operands, operand-preserving or hidden empty displacement controls, and `SYMBOL`,
  remaining display/layout, action/automation fields beyond deterministic quoted/unquoted formatted display text and hidden validated `PRINT` direct/group forms,
  compatibility/private fields including `PRIVATE`/`ADDIN`/`DATA`/`GLOSSARY`/
  `HTMLACTIVEX`, barcode, or legacy form fields outside deterministic
  `w:ffData` checkbox checked/default states, dropdown result/default selections,
  explicit non-empty text-input current values, and empty-current text-input
  default values with no computed result;
  unresolved bookmark scope,
  unresolved or unsupported remaining value-changing REF cases beyond the
  deterministic note/comment-reference mark subset, broader REF semantics,
  remaining missing
  explicit or direct `REF \f` bookmark targets, existing non-note `REF \f`
  targets, missing `NOTEREF` bookmark targets, existing `NOTEREF` bookmark targets without
  body note-reference marks, unsupported `NOTEREF` switches,
  layout-dependent `PAGEREF`,
  missing `TOC \b` scopes,
  and broader TOC field cases while
  preserving cached field-result inline tabs, line breaks, and no-break/soft hyphens;
- populate `FeatureInventory::unsupported_field_reasons` and diagnostics JSON
  with compact counts for `UnknownField`, `UnresolvedBookmark`,
  `UnsupportedSwitch`, and `NoComputedResult` so downstream gates can distinguish
  unsupported classes within the same field kind, including missing `PAGEREF`
  bookmark targets, malformed document-info syntax, explicit and direct
  bookmark-name `REF \d` supported syntax
  with no computed result, missing explicit or direct `REF \f` targets, existing
  explicit or direct non-note `REF \f` no-computed-result cases, missing
  `NOTEREF` targets, existing non-note `NOTEREF` no-computed-result targets, unsupported
  `NOTEREF` switches, missing `TOC \b` scopes, and truly unresolved
  bookmarks.

Acceptance:

- fields are counted by kind;
- hyperlink behavior remains stable;
- `PAGE`, `TOC`, `FILENAME`, `MERGEFIELD`, `REF`, `PAGEREF`, `NOTEREF`, `TC`, `SEQ`,
  document-info/date/stat fields including app-property-backed `EDITTIME`,
  `NUMPAGES`, `NUMWORDS`, `NUMCHARS`, and `TEMPLATE`, dynamic/control fields
  including deterministic literal arithmetic/comparison/scalar-function formula fields with
  finite decimal/scientific numeric literals, finite literal exponentiation, and comma or semicolon function argument separators, simple non-spanning table
  references over existing plain numeric cells and source-order prior computed
  formula-only cells, including direct A1/RnCn cell references plus aggregate
  references over positional `LEFT`/`RIGHT`/`ABOVE`/`BELOW`, current `R`/`C`, A1
  cell/range, and RnCn cell/range references, skipping absent cells in ragged rows and including
  nested aggregate calls inside literal formula expressions, simple separated `\#`
  numeric pictures with quoted or switch-delimited unquoted operands plus compact single-token `\#` numeric pictures,
  literal affixes, single-section leading `+`/`-`
  sign-control items, `x` digit-drop/rounding positions, and sectioned
  positive/negative/zero pictures, plus neutral `\* MERGEFORMAT`/`\* MERGEFORMATINET`/`\* CHARFORMAT`
  formula tails, formula result text-format tails such as `\* Upper`/`\* Lower`/`\* Caps`/`\* FirstCap`,
  and literal/table formula general-number tails such as `\* DollarText` cents output and integer-valued `\* Hex`/`\* OrdText`,
  malformed formula switch syntax reporting `UnsupportedSwitch`,
  and literal `DEFINED(expr)` checks for parser-local literal expressions and source-order prior bookmark names,
  guarded scalar/table numeric `IF` branch selection skips unsupported or span-unsafe unselected branches,
  quoted/unquoted literal `QUOTE`, malformed literal `QUOTE` syntax reporting
  `UnsupportedSwitch`, literal `IF` comparisons, malformed literal `IF`
  syntax reporting `UnsupportedSwitch`, literal `COMPARE` results,
  invalid literal `COMPARE` syntax reporting `UnsupportedSwitch`,
  explicit-default `FILLIN`/`ASK`
  prompt fields with field-result format switches, valid no-default prompt
  fields cached with `NoComputedResult`, malformed prompt field syntax
  reporting `UnsupportedSwitch`, and literal quoted or unquoted `SET` bookmark
  assignments, including multi-token unquoted payloads, feeding later plain `REF`/direct bookmark references and
  source-order bookmark-backed `IF`/`COMPARE`/`NEXTIF`/`SKIPIF` comparisons,
  including numeric comparison for finite numeric bookmark values, malformed
  `SET` syntax reporting `UnsupportedSwitch`,
  malformed merge-control syntax reporting `UnsupportedSwitch`,
  inserted-content fields including
  `DDE`/`DDEAUTO`, mail-merge helper fields, reference/index fields including
  hidden simple literal `RD`/`TA`/`XE` markers,
  numbering/list fields, document-structure fields including computed
  `SECTION`, structurally bounded `SECTIONPAGES`, and deterministic body
  paragraph- and character-style `STYLEREF`, deterministic simple `EQ`
  fractions/radicals/lists/arrays/scripts/integrals/overstrikes, default and custom brackets, boxed operands including nested simple operands, operand-preserving or hidden empty displacement controls, and `SYMBOL`, display/layout fields, action/automation fields including deterministic
  `GOTOBUTTON`/`MACROBUTTON` quoted/unquoted formatted display text and hidden validated `PRINT` direct/group forms with field-result format switches,
  compatibility/private fields, barcode fields, legacy form fields including
  field-result format switches, and unknown fields are distinguishable.
- simple source-order `SEQ` fields compute default next numbers, `\n`, `\r`,
  `\c`, heading-reset `\s` from resolved body heading scopes, hidden `\h`, and
  common number-format switches, malformed `SEQ` syntax reports
  `UnsupportedSwitch`, while remaining valid broader sequence semantics stay
  cached with `NoComputedResult` diagnostics and do not mutate later
  source-order counters.
- deterministic simple literal `RD`, `TA`, and `XE` reference/index marker fields
  validate their filename or literal marker text, including quoted or
  switch-bounded unquoted `TA`/`XE` marker operands, supported marker switches,
  and field-result format switches, then render as hidden output; invalid marker
  syntax reports `UnsupportedSwitch`, while generated bibliography, citation,
  index, and table-of-authorities fields preserve cached text with
  `NoComputedResult` diagnostics until native generation is modeled.
- plain `AUTONUM` fields compute source-order values with common number and
  text format switches and the documented `\s` separator switch, including unquoted or
  quoted one-character separators; standalone plain, neutral,
  common-number-format, or text-format `AUTONUMLGL`, `AUTONUMOUT`, and `BIDIOUTLINE` values compute on the same source-order counter; level-1
  bare `LISTNUM` fields and quoted or unquoted `LISTNUM NumberDefault`/`LegalDefault` fields compute source-order values with common
  number and text format switches, neutral field-format switches, and `\s`
  starts/resets; invalid numbering/list syntax reports `UnsupportedSwitch`,
  while richer `AUTONUMOUT`/`BIDIOUTLINE` outline semantics
  and richer `LISTNUM` levels/named lists preserve cached text with
  `NoComputedResult` diagnostics until richer
  automatic-numbering semantics are modeled.

### 4.4 `.docx` notes

Current `.docx` note parts are parsed into both the read model's appended note
blocks and a semantic note side table.

Implementation:

- parse real `word/footnotes.xml` and `word/endnotes.xml` entries while skipping
  separator boilerplate;
- preserve Word note ids and note kind in `Document::notes()`;
- attach body reference-id anchors when `document.xml` contains matching
  `w:footnoteReference` or `w:endnoteReference` markers, including normalized
  containing top-level body block text around the matched reference marker
  through direct body blocks and accepted-current body-level revision wrappers;
- keep `footnote_text()` and `endnote_text()` as normalized convenience views
  over the parsed note records;
- leave exact character-range note anchoring for a later body-reference model.

Acceptance:

- `.docx` footnotes/endnotes are extractable as typed records;
- flat `text()`/`model()` note behavior remains unchanged;
- note reference ids and containing block text, including accepted-current
  wrapped body references, are stable enough for callers to correlate records
  with body references.

### 4.5 `.docx` text boxes

Current `.docx` body text boxes are parsed into both the normal read model and a
semantic text-box side table.

Implementation:

- parse visible `w:txbxContent` blocks through the same block parser used by
  flat body extraction;
- preserve `mc:AlternateContent` first-branch behavior so DrawingML Choice and
  VML Fallback serializations of the same shape do not duplicate text;
- expose non-empty records through `Document::text_boxes()` with stable
  synthetic ids and visible text;
- follow the accepted/current revision policy through the shared block parser,
  including direct, inserted, and moved-to text boxes while omitting deleted and
  moved-from old text boxes;
- keep `text_box_text()` as a normalized convenience view over those records;
- expose `wp:anchor` geometry, relative z-order, behind/in-front flags,
  enabled `wp:simplePos` absolute points, `wp:effectExtent` visual-effect bounds,
  anchor `dist*` margins, wrap-element `dist*` margins, wrap policy,
  best-effort visible top-level body block page, and containing-block anchor text
  plus zero-width anchor character offsets and DrawingML preset geometry names
  plus simple sRGB solid fill/outline colors and text-bearing shape body text
  through `Document::floating_shapes()` when present, including anchors under
  transparent body-level content-control, custom-XML, smart-tag, single-branch
  `mc:AlternateContent`, and
  accepted/current revision wrappers, with direct, inserted, and moved-to shapes
  surfaced and deleted or moved-from old-only shapes omitted;
- leave exact body anchor ranges, nested/table-specific page resolution, and real
  text-wrap reflow to the floating-shape layout track.

Acceptance:

- `.docx` text-box text is extractable as typed records;
- flat `text()`/`model()` text-box behavior remains unchanged;
- text-box records follow the accepted/current revision policy;
- alternate-content fallback text is not double-counted.

### 4.6 `.doc` subdocuments

Current `.doc` text and model assembly preserve subdocument source spans and
mirror the first recovered default, even-page, and first-page legacy
header/footer variants into global `DocSetup` running slots when `PlcfHdd` story
indexes identify them, and recovered header/footer subdocuments now surface
through `Document::header_footers()` as best-effort records with synthetic ids
and visible text. When legacy `PlcfHdd` story boundaries are available, rdoc
splits stories and classifies exact even-page, odd-page, and first-page
header/footer variants; otherwise it falls back to `Unknown` kind. `.docx`
header/footer
references surface through the same API as exact referenced part records with
stable `part#type` ids and default, even-page, and first-page variants; the
model stores default, first-page, and even-page `.docx` references for paragraph
section breaks and the final section, inherits the previous default when omitted,
and the from-scratch writer emits those variant references.
Recovered annotation
subdocuments now surface through
`Document::comments()` as best-effort comment records with synthetic ids and
visible text, recovered footnote/endnote subdocuments surface through
`Document::notes()` as best-effort note records with synthetic ids, note kind,
and visible text, and recovered text-box subdocuments surface through
`Document::text_boxes()` as best-effort text-box records with synthetic ids and
visible text.
Deepening should continue separating model sections:

- main body;
- headers;
- footers;
- footnotes;
- endnotes;
- text boxes where recoverable.

Implementation:

- preserve CP ranges per subdocument;
- expose exact `.doc` region text APIs over the model region map:
  `main_text`, `header_text`, `annotation_text`, `endnote_text`,
  `text_box_text`, plus `footnote_text` as a footnote+endnote compatibility
  view built from the exact non-contiguous ranges;
- assemble semantic blocks per region and record `DocModel::regions` entries with
  source CP spans, block ranges, visible-text spans, and recovered `PlcfHdd`
  story indexes when available;
- expose safe region queries through `DocModel::source_regions()`,
  `source_region_blocks()`, `source_region_text()`, and
  `source_region_kind_text()`;
- expose non-empty legacy annotation regions through `Document::comments()`
  with best-effort source-region anchors until richer body anchors and author
  metadata can be recovered;
- expose non-empty legacy footnote/endnote regions through `Document::notes()`
  with best-effort source-region anchors until richer note-reference anchors can
  be recovered;
- expose non-empty legacy text-box regions through `Document::text_boxes()`
  with best-effort source-region anchors until richer shape anchors can be
  recovered;
- expose running header/footer records through `Document::header_footers()`,
  using exact `.docx` referenced part ids and default/even/first-page variants,
  selecting default `.docx` references for paragraph section-break setup and the
  final model surface when present, inheriting the previous section default when
  omitted, modeling explicit first/even-page variants, and emitting authored
  first/even variant references plus even/odd settings, with renderer selection
  for section-aware first/even/default running variants, first-page variants
  scoped to each section, and even variants based on emitted page parity,
  exact legacy `.doc` even-page, odd-page, and first-page header/footer variants
  when `PlcfHdd` story indexes identify the record, mirrored into global
  `DocSetup` running slots, and legacy `.doc` `Unknown` kind as the fallback;
- map recovered legacy header/footer variants into `DocSetup` default, even, and
  first running slots, then deepen toward richer legacy multi-section
  header/footer application semantics;
- add diagnostics for flattened regions until complete:
  `LegacyDocFlattenedSubdocuments` reports FIB footnote, header/footer,
  annotation, endnote, and text-box character counts while those ranges still
  remain in the flat block stream or lack dedicated semantic stores.

Acceptance:

- `.doc` `model()` no longer flattens all non-body content without reporting it;
- `main_text`, `header_text`, and full `text` stay explicit.

### 4.7 Core metadata

`Document::core_properties()` exposes a small stable metadata surface:

- `.docx` reads `docProps/core.xml`;
- supported fields are `title`, `subject`, `creator`, `description`,
  `keywords`, `category`, `contentStatus`, `lastModifiedBy`, `created`,
  `modified`, `lastPrinted`, `revision`, and `version`;
- model-backed documents surface generated `DocSetup` core metadata fields
  (`title`, `subject`, `creator`, `description`, `keywords`, `category`,
  `contentStatus`, `lastModifiedBy`, `created`, `modified`, `lastPrinted`,
  `revision`, and `version`);
- `Document::set_core_property()` remains the package-preserving edit path for
  updating the existing text-oriented `.docx` core properties.

## 5. Editor Deepening Plan

### 5.1 Package invariants

Every save must validate:

- all internal relationship targets exist or are intentionally external;
- content types cover all parts;
- duplicate/case-colliding part names are handled deterministically;
- edited XML parts are well-formed;
- part and archive budgets are respected.

The retained OPC package validates this at the commit boundary for metadata the
editor regenerated: touched `.rels` parts resolve internal targets relative to
their source part and reject missing package parts; untouched relationship parts
remain byte-preserved on no-op saves.

### 5.2 Transactional mutation

Multi-part edits use rollback-on-error.

Examples:

- add image: document XML, media part, document rels, content type;
- add comment: comments part, comments rel if missing, document markers,
  optional people/authors metadata later;
- add header: header part, header rel, section property reference, content type.

Generated WordprocessingML text fragments for added/updated comments and
added/replaced footnotes/endnotes reuse the central text escaping rule and must
emit `xml:space="preserve"` on `w:t` when caller text has leading or trailing
whitespace, plus `w:tab`/`w:br` markers for caller tabs/newlines.
Body, accepted-current referenced header/footer, explicit WML part, and
body-table-cell text replacements use the same marker serialization for caller
tabs/newlines while keeping the existing single-part or grouped-run transaction
boundaries.
Cached field-result replacement fragments use the same marker serialization for
caller tabs/newlines in simple and common complex fields.

`Document::edited_parts()` and `DocumentReport.edited_parts` return the sorted
package part names touched by the current edit session, backed by the retained
OPC package's dirty set. This is a reporting surface, not a second transaction
log.

### 5.3 Editing policy

Use explicit capability errors:

- `.doc` in-place editing: unsupported;
- read-only package state: unsupported with reason;
- malformed target XML: refuse edit, allow passthrough save if safe;
- unknown feature conflict: refuse or preserve, never silently drop.

Expose those reasons through `EditCapability.read_only_reasons` and the
`PackageReadOnly` document warning so callers can branch before attempting a
mutation.

## 6. Authoring Deepening Plan

Authoring grows in stages:

1. stabilize existing `DocModel` writer;
2. add builder ergonomics;
3. add section and style APIs;
4. add comments and fields;
5. add content controls and initial bar/column/line/pie chart XML caches;
6. add embedded workbook-backed source data for authored charts;
7. add doughnut chart XML/render support on the shared chart data path;
8. add area chart XML/render support on the shared chart data path;
9. add radar chart XML/render support on the shared chart data path;
10. add scatter chart XML/render support on the shared chart data path;
11. add bubble and 3-D bubble chart XML/render support on the shared chart data path;
12. add surface and 3-D surface chart XML/render support, plus wireframe styling,
    on the shared chart data path;
13. add stock chart XML/render support and 3-D bar/column-family shape styling on the
    shared chart data path;
14. add pie-of-pie and bar-of-pie chart XML/render support on the shared
    chart data path;
15. add 3-D bar, stacked 3-D bar, 100% stacked 3-D bar, 3-D column, stacked 3-D column, 100% stacked 3-D column, 3-D line, and 3-D area chart XML/render support on the shared chart data path;
16. add template fill/mail-merge style helpers.

Step 16 now includes `Document::fill_template_fields()`, a package-preserving
helper that fills tagged plain-text body plus accepted-current referenced
header/footer content controls and cached `MERGEFIELD` results by logical field
name. The lower-level
`Document::fill_content_control_by_tag()` and
`Document::fill_content_controls_by_tag()` helpers remain available when callers
want exact tag-only semantics. These fills preserve `w:sdt` metadata and field
instruction markup without promoting the document through the lossy model, and
the multi-field helpers validate and commit records atomically.

The writer should keep round-trip tests:

```text
DocModel -> write_docx -> Document::open -> DocModel
```

External validation should remain:

- python-docx read checks for basic structures;
- LibreOffice open/convert checks where available;
- Word manual smoke checks documented but not required in CI.

## 7. Renderer Deepening Plan

Keep the renderer feature-gated. Improve in measurable slices.

### R1. Render diagnostics

Return a render report:

```rust
pub struct RenderReport {
    pub pages: usize,
    pub warnings: Vec<RenderWarning>,
    pub unsupported: FeatureInventory,
}
```

Concrete entry points:

- `render_pdf_with_report(&DocModel) -> RenderedPdf`;
- `try_render_pdf_with_report(&DocModel) -> Result<RenderedPdf>`;
- `render_pdf_with_fonts_and_report(&DocModel, &[Vec<u8>]) -> RenderedPdf`;
- `try_render_pdf_with_fonts_and_report(&DocModel, &[Vec<u8>]) -> Result<RenderedPdf>`;
- `Document::to_pdf_with_report() -> RenderedPdf`;
- `Document::try_to_pdf_with_report() -> Result<RenderedPdf>`;
- `Document::to_pdf_with_fonts_and_report(&[Vec<u8>]) -> RenderedPdf`;
- `Document::try_to_pdf_with_fonts_and_report(&[Vec<u8>]) -> Result<RenderedPdf>`.

`RenderedPdf` carries `pdf: Vec<u8>` and the report from the same pagination
pass. Reports built from an opened `Document` reuse `Document::report().features`
so unsupported preserved constructs can become render warnings even when the
lossy model cannot draw them directly; reports also warn when model raster image
bytes are unavailable or present but the PDF backend cannot decode that format,
and render paths append compact placeholder lines for those skipped images. Opened-document
render paths also use that
inventory to draw bounded overlay boxes for recovered `.docx` floating-shape
geometry and anchor layout metadata, including enabled `wp:simplePos` absolute
placement, relative z-order, wrap policy, and best-effort visible top-level body
block page selection, and to surface recovered simple sRGB fill/outline colors
and text-bearing shape body text in preview labels. The feature inventory uses
the same accepted/current revision and single-branch `mc:AlternateContent`
policies for floating-shape counts as the semantic shape reader, so direct,
inserted, and moved-to shapes count, deleted and moved-from old-only anchors or
markers are omitted, and one marker is retained for unrecovered
alternate-content shape placeholders. It appends compact placeholder lines for
charts, OLE objects,
unsupported metafile images, image nodes whose bytes are unavailable, skipped
raster images whose bytes the PDF backend cannot decode, and floating-shape
markers that do not yet have recovered geometry. Metafile package parts are
still unsupported for drawing, but the diagnostics inventory records their path,
WMF/EMF family, stored byte size,
compression flag, and raw-header dimensions when a raw or gzip-wrapped EMF or
placeable WMF header makes that recoverable without full rendering.

Authored `Block::Chart` values are different from preserved foreign chart parts:
the model renderer draws bar, stacked bar, 100% stacked bar, 3-D bar, stacked 3-D bar, 100% stacked 3-D bar, column, stacked column, 100% stacked column, 3-D column, stacked 3-D column, 100% stacked 3-D column, line, markerless line, smooth line, stacked line, 100% stacked line, 3-D line, area,
stacked area, 100% stacked area, 3-D area, stacked 3-D area, 100% stacked 3-D area, radar, radar-with-markers, filled radar, scatter, line-only scatter, smooth scatter, smooth markerless scatter, marker-only scatter, bubble, 3-D bubble, pie, exploded pie, 3-D pie, exploded 3-D pie, doughnut, exploded doughnut, surface, 3-D surface,
high-low-close stock, stock, pie-of-pie, and bar-of-pie charts as native vector preview charts and does not
report them as unsupported. Chart parts
observed only through an opened package feature inventory still use the
preserved-but-unmodeled warning and placeholder path. The authored `.docx`
writer emits the chart XML literal caches and a chart-scoped package
relationship to an embedded `.xlsx` workbook so Word-compatible consumers can
open and edit the chart data.

### R2. Fields

- keep cached field result text by default;
- compute generated running footer page numbers from the PDF page list when
  layout context is available;
- compute body `PAGE` field runs from the emitted PDF page number during the draw
  pass, while preserving cached text in the model and exporters;
- treat field-code `HYPERLINK` runs as link annotations for target/anchor,
  tooltip/frame, and documented `\m`/`\n` no-op switch tails, with malformed
  hyperlink syntax reporting `UnsupportedSwitch`, cached body `FILENAME` with malformed switches reporting
  `UnsupportedSwitch`, cached body `MERGEFIELD` with malformed merge-field names
  or switch tails reporting `UnsupportedSwitch`,
  metadata-backed document-info results, and cached date/stat/unmapped
  document-info field results as supported preview-render
  content;
- compute unambiguous `.docx` `REF` bookmark targets, including Word-generated
  hidden bookmark targets, multi-paragraph bookmark ranges, and simple inline
  tabs, line breaks, no-break/soft hyphens, and deterministic `REF \* Upper`/
  `REF \* Lower`/`REF \* Caps`/`REF \* FirstCap` text format switches,
  source-order `REF \p` relative-position results, explicit numbered-paragraph
  `REF \n` labels from single-branch source paragraphs including `\n \p`,
  `\n \t`, `REF \r` relative-context labels including `\r \p` and `\r \t`,
  and `REF \w` full-context labels
  including `\w \p` relative suffixes and `\w \t` numeric-text suppression,
  `REF \f` visible body footnote/endnote reference marks, body
  comment-reference markers, exact matching bookmarked comment ranges, and
  bookmarks wholly inside body comment ranges,
  including prior generated REF note marks in source order plus common
  field-result number/text format switches,
  text-neutral `REF \d "separator"` bookmark text while value-changing
  sequence/page separator cases preserve cached text, direct bookmark-name field
  computation when the
  bookmark exists with supported text-format switches, neutral `\h`,
  explicit-number `\n`/`\n \t`/`\r`/`\r \t`/`\w`/`\w \t`, note-reference `\f`,
  sequence-separator `\d`, and source-order `\p`,
  bookmarked `NOTEREF`/legacy `FTNREF` footnote/endnote reference marks with
  neutral `\h`, note-reference-style `\f`, source-order `\p`
  above/below results, and common field-result number/text format switches,
  plus bare default `TOC`, standalone bookmark-scoped default `TOC \b`, explicit
  `TOC \o` heading-outline ranges including omitted all-level ranges and common `\o`/`\u` combinations with
  value-neutral `\h`/`\z` switches, text-preserving `\w`/`\x` switches
  normalized to plain text, and text-neutral `\n` no-page-number,
  `\p` entry/page separator, `\s` sequence-number page prefix, and `\d`
  sequence/page separator switches, deterministic TOC `\* Upper`/`\* Lower`/
  `\* Caps`/`\* FirstCap` field-result format switches,
  neutral TOC `\* MERGEFORMAT`/`\* MERGEFORMATINET`/`\* CHARFORMAT`,
  quoted or switch-delimited unquoted `TOC \t` custom-style entries,
  `TOC \f` entries from matching `TC "Text"` markers with optional `\f` type
  identifiers, `\l` levels, and common marker text-format tails, with supported
  `TC` marker fields themselves rendering as hidden output and unsupported `TC`
  marker syntax preserving cached text with `UnsupportedSwitch` diagnostics, `TOC \c` full-caption entries and `TOC \a`
  label/number-omitted caption-text entries from paragraphs containing
  matching `SEQ Identifier` fields, with simple or common complex dirty/stale
  `SEQ` caption numbers recomputed from source order,
  and standalone `TOC \u` fields over explicit paragraph outline levels, plus
  `TOC \b` bookmark-scoped variants when the bookmark range is recoverable,
  including empty computed results for existing scopes with no matching entries,
  body `PAGE` current-page fields with deterministic page-number and common
  field-result format switches in trusted source-marker contexts,
  deterministic display-only body `PAGE` `w:start` restart labels for immediate
  section-start fields after visible intro text,
  deterministic `PAGEREF` `\* Arabic`, `\* alphabetic`/`\* ALPHABETIC`,
  `\* roman`/`\* ROMAN`, `\* Ordinal`, `\* CardText`, `\* OrdText`, and
  `\* Hex`, integer-valued `\* DollarText`, and page-number-only `\* ArabicDash`
  number-format switches plus common field-result format switches when the
  target page is structural from trusted source markers or trusted hard-break
  advancement, honoring deterministic display-only `w:pgNumType w:start`
  page-number restart labels and trusted supported `w:pgNumType w:fmt` styles on structural section
  starts and single-section final `body/sectPr` page-number defaults, and trusted leading-structural or
  source-marker plus deterministic display-only restart and hard-break-after-target `\p`
  relative-position results when target and field page/order are known,
  while normalizing simple inline heading/caption tabs, line breaks,
  no-break/soft hyphens, and supported literal symbols, keep cached
  field-result inline tabs, line breaks, and
  no-break/soft hyphens when computation is unsupported, and surface unresolved
  bookmark scope, unsupported remaining value-changing REF cases beyond the
  deterministic note/comment-reference mark subset, broader REF semantics,
  unresolved or
  unsupported NOTEREF switches, remaining layout-dependent `PAGEREF`, or
  broader TOC/REF cases separately in diagnostics;
- expose unsupported field evaluation warnings for unknown fields beyond named
  document-info/date/stat display fields, dynamic/control fields beyond
  deterministic literal arithmetic/comparison/scalar-function formula fields with
  finite decimal/scientific numeric literals, finite literal exponentiation, and comma or semicolon function argument separators, simple non-spanning table
  references over existing plain numeric cells and source-order prior computed
  formula-only cells, including direct A1/RnCn cell references plus aggregate
  references over positional `LEFT`/`RIGHT`/`ABOVE`/`BELOW`, current `R`/`C`, A1
  cell/range, and RnCn cell/range references, skipping absent cells in ragged rows and including
  nested aggregate calls inside literal formula expressions, simple separated `\#`
  numeric pictures with quoted or switch-delimited unquoted operands plus compact single-token `\#` numeric pictures,
  literal affixes, single-section leading `+`/`-` sign-control items, `x`
  digit-drop/rounding positions, and sectioned
  positive/negative/zero pictures, plus neutral `\* MERGEFORMAT`/`\* MERGEFORMATINET`/`\* CHARFORMAT`
  formula tails, formula result text-format tails such as `\* Upper`/`\* Lower`/`\* Caps`/`\* FirstCap`,
  and literal/table formula general-number tails such as `\* DollarText` cents output and integer-valued `\* Hex`/`\* OrdText`,
  malformed formula switch syntax reporting `UnsupportedSwitch`,
  and literal `DEFINED(expr)` checks for parser-local literal expressions and source-order prior bookmark names,
  guarded scalar/table numeric `IF` branch selection skips unsupported or span-unsafe unselected branches, literal
  quoted/unquoted literal `QUOTE`, literal `IF` finite numeric/text comparisons, literal `COMPARE`
  finite numeric/text results, explicit-default `FILLIN`/`ASK` prompt fields,
  valid no-default prompt fields cached with `NoComputedResult`, and literal quoted or unquoted
  `SET` bookmark assignments, including multi-token unquoted payloads, with field-result format switches feeding later
  plain `REF`/direct bookmark references and source-order bookmark-backed
  `IF`/`COMPARE`/`NEXTIF`/`SKIPIF` comparisons, including numeric comparison
  for finite numeric bookmark values, plus literal `NEXT` and literal or source-order
  bookmark-backed `NEXTIF`/`SKIPIF` merge-control fields with field-result format
  switches that render as hidden output without running a mail merge,
  inserted-content fields, mail-merge helper
  fields, reference/index fields beyond hidden simple literal `RD`/`TA`/`XE` markers,
  numbering/list fields, document-structure
  fields beyond computed `REVNUM`/`SECTION`, structurally bounded `SECTIONPAGES`, and
  deterministic body paragraph- and character-style `STYLEREF`,
  hidden validated `ADVANCE`, deterministic simple `EQ` fractions/radicals/lists/arrays/scripts/integrals/overstrikes, default and custom brackets, boxed operands including nested simple operands, operand-preserving or hidden empty displacement controls, and `SYMBOL`,
  remaining display/layout fields,
  action/automation fields beyond deterministic quoted/unquoted formatted display text and hidden validated `PRINT` direct/group forms,
  compatibility/private fields, barcode fields, and legacy form fields beyond
  deterministic `w:ffData` checkbox checked/default states, dropdown
  result/default selections, explicit non-empty text-input current values, and
  empty-current text-input default results;
  named dynamic/control fields beyond deterministic literal arithmetic/comparison/scalar-function formula
  fields with comma or semicolon function argument separators, simple non-spanning
  table direct A1/RnCn cell references plus aggregate references over existing
  plain numeric positional `LEFT`/`RIGHT`/`ABOVE`/`BELOW`, current `R`/`C`, A1
  cell/range, and RnCn cell/range references, skipping absent cells in ragged rows and including
  nested aggregate calls inside literal formula expressions, simple separated `\#`
  numeric pictures with quoted or switch-delimited unquoted operands plus compact single-token `\#`
  numeric pictures, literal affixes, single-section leading `+`/`-` sign-control items, `x`
  digit-drop/rounding positions, and sectioned positive/negative/zero pictures,
  plus neutral `\* MERGEFORMAT`/`\* MERGEFORMATINET`/`\* CHARFORMAT` formula tails,
  formula result text-format tails such as `\* Upper`/`\* Lower`/`\* Caps`/`\* FirstCap`, and
  literal/table formula general-number tails such as `\* DollarText` cents output and integer-valued `\* Hex`/`\* OrdText`,
  malformed formula switch syntax reporting `UnsupportedSwitch`,
  literal `DEFINED(expr)` checks for parser-local literal expressions and source-order prior bookmark names,
  guarded scalar/table numeric `IF` branch selection skips unsupported or span-unsafe unselected branches,
  quoted/unquoted literal `QUOTE`, literal `IF` comparisons, literal `COMPARE` results,
  explicit-default `FILLIN`/`ASK` prompt fields, valid no-default prompt
  fields cached with `NoComputedResult`, and
  literal quoted or unquoted `SET` bookmark assignments, including multi-token unquoted payloads, with field-result
  format switches feeding later plain `REF`/direct bookmark references and
  source-order bookmark-backed `IF`/`COMPARE`/`NEXTIF`/`SKIPIF` comparisons,
  including numeric comparison for finite numeric bookmark values, with malformed
  `SET` syntax reporting `UnsupportedSwitch`, plus literal `NEXT` and literal
  or source-order bookmark-backed `NEXTIF`/`SKIPIF` merge-control fields with field-result format
  switches that render as hidden output without running a mail merge,
  inserted-content, mail-merge helper, reference/index beyond hidden simple
  literal `RD`/`TA`/`XE` markers, numbering/list,
  document-structure fields beyond computed `REVNUM`/`SECTION`, structurally bounded
  `SECTIONPAGES`, and deterministic body paragraph- and character-style
  `STYLEREF`, hidden validated `ADVANCE`, deterministic simple `EQ` fractions/radicals/lists/arrays/scripts/integrals/overstrikes, default and custom brackets, boxed operands including nested simple operands, operand-preserving or hidden empty displacement controls, and `SYMBOL`,
  remaining display/layout, action/automation fields beyond deterministic quoted/unquoted formatted display text and hidden validated `PRINT` direct/group forms,
  compatibility/private fields including `PRIVATE`/`ADDIN`/`DATA`/`GLOSSARY`/
  `HTMLACTIVEX`, barcode, or legacy form fields outside deterministic
  `w:ffData` checkbox checked/default states, dropdown result/default selections,
  explicit non-empty text-input current values, and empty-current text-input
  default values with no computed result,
  unresolved bookmark
  scope, unresolved or unsupported remaining value-changing REF cases beyond the
  deterministic note/comment-reference mark subset, broader REF semantics,
  unresolved or unsupported NOTEREF switches, remaining layout-dependent
  `PAGEREF`, and broader TOC fields, with
  `unsupported_field_reasons` reason counts in the feature inventory JSON.

### R3. Floating shapes

- detect anchored/floating shapes and capture `.docx` `wp:anchor` geometry plus
  layout metadata (`wp:extent`, `wp:docPr`, `wp:positionH`, `wp:positionV`,
  `wp:simplePos` when enabled by `wp:anchor/@simplePos`, `wp:effectExtent`,
  `relativeHeight`, `behindDoc`, `layoutInCell`, `locked`, `allowOverlap`,
  `wp:anchor/@dist*`, `wp:wrap*` kind/text, `wp:wrap*/@dist*`,
  `wp:wrapPolygon` point lists, visible top-level body block anchor index,
  containing-block anchor text, zero-width anchor character offset, and
  `a:prstGeom/@prst` preset geometry, simple sRGB solid fill/outline colors,
  and text-bearing `w:txbxContent`, including transparent body-level
  content-control, custom-XML, smart-tag, single-branch `mc:AlternateContent`,
  and accepted/current revision wrappers, while omitting deleted and moved-from
  old-only shapes);
- emit compact summary placeholders for shape markers without recovered geometry,
  backed by the same feature inventory;
- draw recovered `.docx` anchors as bounded approximate overlay boxes using EMU
  extents, enabled `wp:simplePos` absolute placement or simple
  `positionH`/`positionV` offset/alignment policy, recovered relative z-order,
  recovered visible top-level body block page, and compact anchor-text preview
  labels that include recovered effect extents, anchor distance margins, and
  wrap-element distance margins plus wrap-polygon point counts when present;
- keep exact body anchor range/page resolution, real text-wrap reflow, deeper
  z-order semantics, and full non-text Office-Art drawing as later layout work;
- validate against golden fixtures.

### R4. Font and script support

- bundled font option behind a feature or explicit font registration;
- extend the partial common Symbol/Wingdings mapping beyond current
  display/render code points;
- CJK fallback stability;
- RTL as a separate milestone, not incidental line-breaking.

### R5. Layout validation

Metrics:

- PDF parses;
- selectable text extraction recall;
- page-count ratio;
- image/golden hash for selected public fixtures;
- warning count trend.

`examples/to_pdf --report-json` writes the `RenderReport` for one render, and
`scripts/render_validate.py --json` is the release-friendly aggregate output for
these metrics, including render-warning counts/kinds and a compact `gate`
section. The default gate fails when any measured document is below
`--recall-min`; release jobs can additionally enforce aggregate thresholds for
mean recall, minimum/maximum mean page ratio, mean aHash similarity, mean warning
count, and skipped files. The named `public-release` policy records `0.97`
per-document recall, `0.90` aggregate mean recall, and `0` skipped files as the
selected optional local render thresholds. The table view remains for local
iteration.

Near-term release validation is maintenance-only. Do not add new render
validation metrics, benchmark normalization, evidence formats, release-policy
thresholds, or manifest hardening unless an existing gate regresses, a blocker
breaks release automation, or the user explicitly starts a release task. Treat
strict release evidence generation as a final public-readiness pass after the
engine slices are closed.

## 8. Validation Infrastructure

### 8.1 Public corpus

Keep committed fixtures redistributable and synthetic or clearly licensed.

Fixture classes:

- minimal `.docx`;
- styles and numbering;
- tables with spans and nesting;
- headers/footers;
- footnotes/endnotes;
- text boxes;
- comments;
- tracked changes;
- fields;
- images;
- malformed but bounded inputs;
- legacy `.doc` samples where license permits.

### 8.2 Private corpus hooks

Scripts may accept:

- `--corpus`;
- `RDOC_BENCH_CORPUS`;
- `RDOC_RENDER_CORPUS`;
- `RDOC_PRIVATE_FIXTURES`.

Private paths must never be defaults.

### 8.3 Differential checks

Use external tools as optional validators:

- python-docx for `.docx` structural sanity;
- Apache POI for `.doc` text comparison where locally configured;
- LibreOffice for openability/render comparison;
- PDF text extraction for renderer recall.

External checks should skip cleanly if tools are missing.

This validation infrastructure is currently parked except for regressions,
broken gates, or explicit release tasks. Engine work should not spend additional
cycles tightening release evidence while R2 field semantics, legacy `.doc`
anchors/header-footer behavior, and public behavior fixtures remain open.

Release jobs should publish a machine-readable artifact manifest generated by
`scripts/release_manifest.py`: every file records byte size and SHA-256, and
public hygiene audit reports, public corpus TSV manifests, validation reports,
and extraction benchmark reports are embedded by summary plus compact gate
metadata only so downstream users can verify the artifacts and reproduce the
corpus checks without ingesting large row data. Corpus TSV summaries reject
empty manifests, duplicate columns or paths, negative numeric counts, and
duplicate warning tokens before embedding totals. The manifest can also embed the
named `public-release` policy, which records the public hygiene audit, required
Rust gates, and optional local render/extraction threshold values. A compact
`release_evidence` section records a strict-policy status, whether strict local
evidence enforcement was enabled, which evidence paths were provided, whether
the strict public-release input set resolves to existing valid reports and
manifests, and which strict inputs are still missing, including missing/invalid
report files, invalid public corpus manifests, and manifest pairs whose document
path lists do not match or whose listed documents are absent. Tagged release automation intentionally
emits the non-strict policy manifest until local render and extraction reports
are generated in the workflow.
When invoked
with `--enforce-policy-inputs`, the manifest generator requires a passing public
hygiene report, the local render-validation report, at least one extraction
benchmark report identified as `rdoc.benchmark-report.v1` /
`extract-vs-mature`, and exactly the public `MANIFEST.tsv` plus
`RENDER_MANIFEST.tsv` corpus manifest pair with matching document paths whose
listed documents exist, then
rejects hygiene, validation, or benchmark reports whose compact `gate.passed` is
not true or whose recorded thresholds are weaker than the named `public-release`
policy.
The public hygiene audit also scans bounded decoded byte text views from legacy
`.doc` files and rejects oversized legacy binaries rather than passing them
uninspected. For Office OPC packages such as `.docx` and `.xlsx`, it scans
internal member paths plus textual parts such as core properties, relationships,
content types, WordprocessingML XML, and embedded Office package XML such as
chart workbooks for release blockers, while leaving binary media payloads
opaque.
The tag-driven release workflow runs `scripts/public_hygiene_audit.py` before the
required Rust gates, packages the crate, generates this manifest against the
packaged `.crate` artifact plus the public hygiene and corpus manifests, and
uploads the manifest, hygiene report, and crate package as workflow artifacts
before publishing to crates.io.

### 8.4 Fuzzing

Targets:

- `.doc` open;
- `.docx` open;
- OPC package parse;
- XML tree parse/serialize;
- `replace_body_text`;
- `set_field_result`;
- `fill_content_control_by_tag`;
- `fill_content_controls_by_tag`;
- `fill_template_fields`;
- `accept_all_revisions`;
- `reject_all_revisions`;
- `set_hyperlink_target`;
- `set_comment_text`;
- `add_comment_on_text`;
- `set_table_cell_text`;
- `add_footnote_on_text`;
- `add_endnote_on_text`;
- `replace_header_footer_text`;
- `replace_text_in_part`;
- `replace_note_text`;
- `add_image_png`;
- `replace_image_png`;
- `add_image_jpeg`;
- `replace_image_jpeg`;
- `add_image_gif`;
- `replace_image_gif`;
- `add_image_bmp`;
- `replace_image_bmp`;
- `add_image_tiff`;
- `replace_image_tiff`;
- `add_image_webp`;
- `replace_image_webp`;
- `set_core_property`;
- render from arbitrary bounded `DocModel`.

Fuzz findings should become regression tests when practical.

## 9. CI and Release Gates

Required default gate:

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --doc
```

Required render gate:

```text
cargo test --all-targets --features render
```

Optional local gates:

```text
python scripts/bench_vs_mature.py --corpus <path> --json \
  --min-poi-recall-mean 0.95 --min-poi-f1-mean 0.95 --max-errors 0 --min-scored 1 \
  --output dist/extract-benchmark.json
python scripts/render_validate.py --json --min-mean-recall 0.90 --max-skipped 0 \
  corpus/public/**/*.docx > render.json
python scripts/release_manifest.py --corpus-manifest corpus/public/MANIFEST.tsv \
  --corpus-manifest corpus/public/RENDER_MANIFEST.tsv \
  --release-policy public-release \
  --enforce-policy-inputs \
  --validation-report render.json --benchmark-report dist/extract-benchmark.json \
  --output dist/rdoc-release-manifest.json dist/*
python scripts/validate_edit_check.py --corpus <path>
cargo fuzz run <target>
```

Release notes must include:

- shipped features;
- known unsupported features;
- corpus and validation scope;
- renderer limitations;
- any format-specific caveats.

## 10. Sequencing

Implement in this order:

1. diagnostics/report API;
2. public corpus expansion;
3. `.docx` comments and fields read model;
4. tracked-change read policies;
5. `.doc` region split;
6. package edit transactions;
7. authoring builder ergonomics;
8. renderer report and field warnings;
9. floating shape placeholders;
10. thin WASM read/report adapter over the same core;
11. static browser inspector for open, inspect, extract, and preview;
12. browser editing only after native edit transactions and diagnostics remain robust.

This order makes every deeper feature visible and testable before it becomes an
editing or rendering promise.

Current implementation focus should treat the broad sequence above as mostly
established infrastructure and promote only bounded compatibility slices:

- R2-a: keep shared field parsers aligned across evaluator, document-report
  diagnostics, and render-model diagnostics when exact duplicated logic or
  report/evaluator drift is proven. `PAGEREF`, `REF`, `NOTEREF`/`FTNREF`, and
  TOC now have focused document-report/render-model parity tests for computed
  and cached-gap buckets, and empty unsupported simple/complex field
  instructions plus supported hidden `RD`/`TA`/`XE` marker fields remain
  reportable in model/render inventories. New R2-a work should start from
  concrete uncovered drift;
- R2-b: promote `PAGE` and `PAGEREF` semantics only for deterministic layout
  contexts already represented by reader or renderer evidence, including
  paragraph-end section-break targets;
- R2-c: promote `REF`, direct bookmark references, `NOTEREF`/`FTNREF`, and TOC
  semantics only for deterministic source-order, note-mark, numbering, and
  scope contexts;
- R2-d: preserve cached field text and report `UnsupportedSwitch`,
  `NoComputedResult`, or `UnresolvedBookmark` for valid but unresolved
  compatibility cases and non-deterministic field families;
- R2-e: defer legacy `.doc` exact anchors and richer multi-section
  header/footer semantics beyond recovered global running variants until public
  synthetic fixtures and expected contracts exist;
- defer floating-shape wrap/reflow, extension chart families, and metafile
  drawing until public synthetic fixtures and expected contracts exist;
- keep release evidence machine-readable and separate from private corpus data,
  but do not expand release policy or validation formats during the near-term
  engine push unless fixing a regression or explicit release blocker.
