# Changelog

All notable changes to `rdoc` are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Custom properties in diagnostics.** `DocumentReport` and diagnostics JSON
  now expose parsed string custom document properties alongside core metadata.
- **Floating-shape overlay ordering.** PDF previews now draw recovered
  `behindDoc` floating-shape overlays before page text, while front overlays
  still draw above content.
- **PAGE field-result formatting.** Trusted deterministic `.docx` body `PAGE`
  current-page text now combines supported page-number formats with common
  `\* Upper`/`\* Lower`/`\* Caps`/`\* FirstCap` field-result switches.
- **PAGEREF field-result formatting.** Deterministic `.docx` `PAGEREF`
  page-reference text now combines supported page-number formats with common
  `\* Upper`/`\* Lower`/`\* Caps`/`\* FirstCap` field-result switches.
- **PAGEREF final-section page numbering.** Trusted `.docx` `PAGEREF`
  targets in single-section documents now honor final `body/sectPr`
  `w:pgNumType w:start` and supported `w:fmt` page-number defaults when an
  explicit source page marker already makes the target page unambiguous.
- **Field result marker fidelity.** `Document::set_field_result()` now writes
  user-supplied tabs/newlines as WordprocessingML `w:tab`/`w:br` markers for
  both simple and common complex cached field results.
- **Text replacement marker fidelity.** `Document::replace_body_text()`,
  `Document::replace_header_footer_text()`, `Document::replace_text_in_part()`,
  and `Document::set_table_cell_text()` now write user-supplied tabs/newlines as
  WordprocessingML `w:tab`/`w:br` markers instead of literal control characters
  inside `w:t`.
- **NOTEREF field-result formatting.** Deterministic `NOTEREF`/`FTNREF`
  footnote/endnote mark and source-order relative-position results now apply
  common `\* Upper`/`\* Lower`/`\* Caps`/`\* FirstCap` field-result switches,
  while unresolved targets and unsupported switches remain cached and
  diagnostic.
- **Standalone `AUTONUMLGL`/`AUTONUMOUT` fields.** Plain automatic-numbering
  variants now compute source-order values on the same deterministic counter as
  `AUTONUM`, while richer outline formatting remains cached and diagnostic.
- **Formatted `BIDIOUTLINE` diagnostics.** `BIDIOUTLINE` fields with valid
  field-result format switches now preserve cached text with `NoComputedResult`
  diagnostics, while malformed format switches remain `UnsupportedSwitch`.
- **Unquoted single-token `SET` fields.** Deterministic `SET Bookmark Value`
  fields now accept a single unquoted literal token, feed later plain
  `REF`/direct bookmark references, and still reject ambiguous multi-token
  unquoted assignments.
- **Legacy `FORMTEXT` current values.** Non-empty `FORMTEXT` result text is now
  materialized as the computed value instead of being reported as unsupported;
  empty-current text-input defaults, checkbox states, and dropdown selections
  keep their existing deterministic legacy-form behavior.
- **Legacy form fields in notes and headers/footers.** `.docx` footnote,
  endnote, header, and footer legacy form fields now use the same deterministic
  value computation and protected-form cached-text policy as body fields, and
  appear in `Document::fields()` with matching diagnostics.
- **3-D bar/column shape styling.** `ChartBuilder::shape(ChartShape::...)`
  now emits Word-compatible `c:shape` values such as `cylinder` and `pyramid`
  for authored 3-D bar and 3-D column charts and renders approximate shaped
  native previews without preserved-chart warnings.
- **Surface chart wireframe styling.** `ChartBuilder::wireframe()` now emits
  `c:wireframe val="1"` for authored surface and 3-D surface charts and draws a
  wireframe-style native preview without preserved-chart warnings.
- **3-D surface chart authoring and preview.** `ChartBuilder::surface_3d()`
  emits workbook-backed OOXML `c:surface3DChart` parts, reopens through the
  normal package path, and renders through the native chart-preview pipeline
  without preserved-chart warnings.
- **3-D line and area chart authoring and preview.** `ChartBuilder::line_3d()`
  and `ChartBuilder::area_3d()` emit workbook-backed OOXML `c:line3DChart`
  and `c:area3DChart` parts, reopen through the normal package path, and render
  through the native chart-preview pipeline without preserved-chart warnings.
- **3-D bar and column chart authoring and preview.** `ChartBuilder::bar_3d()`
  and `ChartBuilder::column_3d()` emit workbook-backed OOXML `c:bar3DChart`
  parts, reopen through the normal package path, and render through the native
  chart-preview pipeline without preserved-chart warnings.
- **3-D pie chart authoring and preview.** `ChartBuilder::pie_3d()` emits
  workbook-backed OOXML `c:pie3DChart` parts, reopens through the normal package
  path, and renders through the native chart-preview pipeline without
  preserved-chart warnings.
- **Comment/note edit whitespace preservation.** Newly created `.docx`
  comments, updated comments, and newly created or replaced footnote/endnote
  text now mark generated `w:t` runs with `xml:space="preserve"` when
  user-supplied text has leading or trailing whitespace and emit `\t`/`\n` as
  WordprocessingML tab/break markers.
- **Comment tab/break fidelity.** `.docx` comment extraction now treats
  `w:tab`, `w:br`, and `w:cr` markers as visible tab/newline text in both
  comment bodies and anchors, and authored comments emit `\t`/`\n` as
  WordprocessingML markers.
- **Package-preserving `.docx` editing (`Document::open` → edit → `save`).** rdoc is
  now an *editor*, not only a generator: opening a `.docx` retains the whole OPC
  package, and `Document::save() -> Result<Vec<u8>>` re-emits it with every part it
  doesn't model preserved verbatim — themes, settings, fonts, comments, custom XML,
  charts, embeddings, and unknown/future parts. A no-op open→save is byte-stable per
  part. Two edit surfaces (see `docs/prd-rdoc-write-edit.md` / `docs/trd-…`):
  - **Model overlay (A)** — `Document::body_mut()` edits the rich `DocModel`; `save`
    regenerates only `word/document.xml` and merges relationships (preserving the
    original's satellite rels, re-minted to non-colliding `rId`s). Note: regenerating
    the body from the lossy model does not preserve unmodeled *body* elements.
  - **Element-tree edit (B)** — `Document::replace_body_text` and
    `Document::add_image_png` mutate the live `document.xml` element tree, so fields,
    content controls (`w:sdt`), `mc:AlternateContent` shapes, comments, and tracked
    changes are **preserved**; image insertion reconciles the media part,
    content-type, and a fresh `rId` transactionally. Lazy promotion means only the
    edited part re-serializes; every other part stays byte-identical.
  - **New documents** — `Document::new()` opens a bundled blank template (à la
    python-docx's `default.docx`); `try_write_docx` is a fallible `write_docx`.
  New internals: `opc` (OPC round-trip layer — parts/content-types/rels graph +
  `rId` allocator) and `xmltree` (a faithful, edit-preserving arena XML tree).
  Validated on the 127-file corpus with python-docx as the strict external checker:
  passthrough byte-stable per part; A-overlay and B-image edits produce packages
  python-docx opens (B: inline image present) on every openable file.
- **Render fidelity & extraction depth.** Run-level
  `mc:AlternateContent` shapes are extracted (floating text boxes no longer
  dropped); `.docx` autonumber labels are computed from `numFmt`/`lvlText`/`start`
  (1./a)/1.1/i. instead of decimal-only); footnotes/endnotes are read; `w:caps`/
  `w:smallCaps` render uppercased; and the renderer is **model-driven** for page
  size, orientation, and per-side margins (Letter/A3/landscape/sidebar layouts
  paginate correctly instead of a forced A4). A whole-archive media budget caps
  cumulative image inflation. Across these, `.docx` render recall vs LibreOffice
  rose to ~0.93 and `.doc` page counts line up; see the *Scope & parity* notes
  for the remaining renderer limitations.
- **Wrapper-aware floating-shape anchors.** `.docx` `wp:anchor` extraction now
  resolves anchor block index/text through transparent body-level content
  controls and custom XML wrappers, records the zero-width anchor character
  offset inside normalized containing-block text, captures DrawingML
  `a:prstGeom/@prst` preset geometry plus simple sRGB solid fill/outline colors
  for textless Office-Art shapes, records enabled `wp:simplePos` absolute points,
  captures `wp:effectExtent` visual-effect bounds, records wrap-element `dist*`
  text-distance margins, matches the visible body blocks used by the model, and
  improves preview overlay page selection for those wrapped anchors.
- **`.docx` headers/footers + text boxes are now read and rendered.** The reader
  resolves running headers/footers from the body `sectPr` references into
  paragraph section-break setup plus final `DocSetup` default, first-page, and
  even-page header/footer variants, using the previous section's default when a
  later section omits it. `header_footers()` exposes the exact referenced
  `part#type` records, each part with its own rels/media, de-duplicated, and
  authored `.docx` output now emits first/even variant refs plus the needed
  settings marker for even-page headers, and the renderer selects section-aware
  first/even/default running variants, with first-page variants scoped to each
  section and even variants based on emitted page parity,
  extracts text-box text (`w:txbxContent`, DrawingML & VML, single branch on
  `mc:AlternateContent`) into the body, and the renderer draws the headers/footers
  in every page margin and flattens nested-table cells. `text()` now includes
  headers/footers; `main_text()` is body-only; `header_text()` works for `.docx`.
  Lifted mean rendered-PDF recall vs LibreOffice on feature-bearing corpus docs
  from 0.687 to 0.932. The text-box recursion is depth-bounded (no stack overflow
  on hostile nesting).
- **`.docx` authoring (`rdoc::write_docx`)** — build a `DocModel` from data and
  serialize a clean, Office-openable `.docx`. Emits rich `w:rPr` (font, half-point
  size, color, highlight, small-caps, super/subscript), `w:pPr` (named heading
  styles via a synthesized `styles.xml` with `outlineLvl`, alignment, spacing,
  indent, shading), bordered tables with per-cell shading / `tcW` width / vertical
  alignment, real image extents, page setup (`sectPr`), and running headers/footers
  with a `PAGE` field. Round-trips through the reader; opens in Word & LibreOffice.
  See `examples/report.rs`.
- **PDF rendering (`rdoc::render_pdf`, `render` feature)** — native typesetting with
  `parley` (Korean/CJK line-breaking + script font fallback) and `krilla` (subset
  embedded fonts, selectable text). Honors run color/size/font, lists + indentation,
  bordered tables with shaded vertically-aligned cells and authored column widths,
  images (PNG/JPEG/GIF/WebP), and clickable hyperlink annotations; multi-page tables
  repeat header rows and oversized rows split across pages.
  `render_pdf_with_fonts` registers caller-supplied fonts for headless/server use.
- **Richer read model** — `CharProps` now carries font/size/color/highlight/
  vert-align/small-caps (incl. `.doc` CHPX `sprm` decoding + the `SttbfFfn` font
  table); `ParaProps` gains spacing/indent/shading; `Cell`/`Table` gain shading,
  vertical alignment, and column widths; new `DocSetup`/`PageSetup`. All additive
  and `Default`, so existing read paths are unaffected.
- **Validation** — `scripts/render_validate.py` (recall / page-count / visual-hash
  vs LibreOffice), a `render` fuzz target, and an integration test of the public
  authoring/render API.
- **PAGE field evaluation** — body `PAGE` fields now compute current page
  numbers from trusted leading structural or source-rendered `.docx` contexts,
  including section page-number restarts/styles and deterministic page-number
  format switches. Visible-content manual-break and broader layout-derived
  current-page cases preserve cached text and now report `NoComputedResult`
  diagnostics.
- **Named `PAGEREF` diagnostics** — `FieldKind::PageRef` now distinguishes
  Word `PAGEREF` fields from unknown fields, preserves their cached page-reference
  text, computes page numbers only when leading explicit page breaks before
  visible body content, enabled paragraph `w:pageBreakBefore`, explicit or
  default `nextPage`, and explicit `evenPage`/`oddPage` section starts in
  leading or trusted rendered context, including trusted `w:pgNumType w:start`
  displayed page-number restarts and supported `w:pgNumType w:fmt` styles
  (decimal variants, enclosed decimal variants, Korean variants,
  lower/upper letter, lower/upper roman, ordinal, cardinal text, and ordinal
  text) on those section starts, source-persisted
  `w:lastRenderedPageBreak` markers, or
  explicit hard breaks after a trusted leading/rendered page context make the
  target bookmark page structural, applies deterministic `\* Arabic`,
  `\* alphabetic`/`\* ALPHABETIC`, `\* roman`/`\* ROMAN`, `\* Ordinal`,
  `\* CardText`, `\* OrdText`, and page-number-only `\* ArabicDash`
  number-format switches plus common field-result format switches, computes
  `\p` relative results from trusted leading
  structural page context or source-persisted rendered page-break/source-order
  hints (`above`, `below`, or `on page N`), and reports missing `PAGEREF`
  bookmark targets as `UnresolvedBookmark` separately from remaining
  layout-dependent page references, target-section numbering styles that are
  still unsupported for target-derived formatting, and unknown fields.
  Compatibility note: downstream code
  with exhaustive matches on the public `FieldKind` enum must add a `PageRef`
  arm or wildcard fallback.
- **REF relative-position and numbered-paragraph fields** — `.docx`
  `REF Bookmark \p` now computes `above`/`below` from unambiguous source-order
  bookmark and field positions, and `REF Bookmark \n` computes explicit
  numbered-paragraph bookmark labels from `word/numbering.xml`, including
  `\n \p` relative suffixes, `\n \t` numeric-text suppression, and
  `REF Bookmark \r` relative-context numbered labels with `\r \p` relative
  suffixes and `\r \t` numeric-text suppression when the REF field paragraph
  also has an unambiguous numbering context, plus
  `REF Bookmark \w` full-context numbered labels with `\w \p` relative suffixes
  and `\w \t` numeric-text suppression,
  `REF Bookmark \f` computes visible body footnote/endnote reference marks
  when the bookmark encloses a body note reference, including prior generated
  `REF \f` marks in source order,
  `REF Bookmark \d "separator"` is accepted as supported syntax and preserves
  cached text until sequence/page separator semantics are modeled, reporting
  `NoComputedResult` separately from unresolved bookmarks, missing explicit or
  direct `REF \f` bookmark targets report `UnresolvedBookmark`, while broader
  existing non-note `REF \f` cases report `UnsupportedSwitch`,
  and direct bookmark-name fields such as `{ Figure1 }` are treated as supported
  `REF`-equivalent fields when the named bookmark exists, including the
  supported `\h`, `\n`, `\n \t`, `\r`, `\r \t`, `\w`, `\w \t`, `\f`, `\d`, `\p`, and `\* Upper`/`\* Lower`/`\* Caps`/`\* FirstCap`
  switches. Existing-bookmark direct fields with uncomputed `\d` or non-note
  `\f` now remain classified under `REF`, preserving cached text while reporting
  the same `UnresolvedBookmark`/`NoComputedResult`/`UnsupportedSwitch` reason
  split as explicit `REF` fields, and diagnostics still preserve remaining value-changing REF
  cases such as comment/annotation insertion and broader REF semantics.
- **NOTEREF field evaluation** — `FieldKind::NoteRef` now distinguishes Word
  `NOTEREF` fields, plus legacy `FTNREF`, from unknown fields. `.docx`
  `NOTEREF Bookmark`, `\h`, `\f`, and `\p` compute footnote/endnote reference
  marks or source-order `above`/`below` results when the bookmark encloses a body
  note reference mark, with common `\* Upper`/`\* Lower`/`\* Caps`/`\* FirstCap`
  field-result switches applied to deterministic output. Diagnostics now distinguish missing `NOTEREF` bookmark
  targets as `UnresolvedBookmark`, existing bookmarks without a body note-reference
  mark as `NoComputedResult`, and unsupported switches as `UnsupportedSwitch`.
  Compatibility note: downstream code with exhaustive
  matches on the public `FieldKind` enum must add a `NoteRef` arm or wildcard
  fallback.
- **Public release policy metadata** — `scripts/release_manifest.py` can embed the
  named `public-release` policy with required Rust gates and selected optional
  render/extraction thresholds, and `--enforce-policy-inputs` turns local report
  inputs plus the exact public `MANIFEST.tsv`/`RENDER_MANIFEST.tsv` corpus pair
  into required passing evidence for strict public manifests; the tag release
  workflow now runs fmt, clippy, default tests, doc tests, and render-feature
  tests before packaging.
- **TOC switch policy** — computed `.docx` `TOC` fields now accept Word's
  text-preserving `\w` and `\x` switches as non-blocking for plain-text computed
  results, `\s Identifier` sequence-number page prefixes as page-number-only
  syntax, range-less `TOC \o` fields as all heading-outline levels, and
  standalone `TOC \b Bookmark` fields as bookmark-scoped default heading TOCs,
  while still normalizing tabs and line breaks in heading text. Diagnostics now
  report missing `TOC \b` scopes as `UnresolvedBookmark` and existing scopes
  with no matching entries as `NoComputedResult`.
- **SECTION and bounded SECTIONPAGES field evaluation** — `.docx` `SECTION`
  fields now compute the current structural section number from paragraph
  `w:sectPr` breaks, and `SECTIONPAGES` computes structurally bounded section
  page counts when the count is source-only and does not require layout. Both
  support simple and common complex fields with neutral/general numeric format
  switches, and replace stale cached text in the read model. Layout-derived
  `SECTIONPAGES` cases remain cached unless covered by a more specific evaluator
  below.
- **STYLEREF paragraph- and character-style evaluation** — `.docx` `STYLEREF`
  fields now compute deterministic body paragraph-style text by matching style id
  or style name, searching backward from the field and falling forward when no
  earlier match exists, and also compute body character-style run text in source
  order, including same-paragraph backward lookup. Simple and common complex
  fields support neutral/general text format switches. Deterministic numbered
  source paragraphs also compute `\n`, `\r`, `\w`, and numeric-text `\t` results
  from the existing numbering context. Page-aware/header-footer lookup and
  layout-dependent variants still preserve cached text with diagnostics.
- **TC-based TOC entries** — `FieldKind::TocEntry` now distinguishes Word `TC`
  table-of-contents entry fields from unknown fields, and `TOC \f` can compute
  plain-text results from matching `TC "Text"` markers with optional `\f` type
  identifiers and `\l` levels. Compatibility note: downstream code with
  exhaustive matches on the public `FieldKind` enum must add a `TocEntry` arm or
  wildcard fallback.
- **SEQ-based TOC captions** — `FieldKind::Sequence` now distinguishes Word
  `SEQ` fields from unknown fields, `TOC \c "Identifier"` can compute
  plain-text full-caption lists, and `TOC \a Identifier` can compute
  label/number-omitted caption-text lists from paragraphs containing matching
  cached `SEQ` fields. Compatibility note: downstream code with exhaustive matches on the
  public `FieldKind` enum must add a `Sequence` arm or wildcard fallback.
- **Document-info display fields** — `FieldKind::DocumentInfo(String)` now
  distinguishes common cached display fields such as `DATE`, `TIME`, `AUTHOR`,
  `TITLE`, `SUBJECT`, `KEYWORDS`, `COMMENTS`, `LASTSAVEDBY`, `DOCPROPERTY`,
  `DOCVARIABLE`, `EDITTIME`, `FILESIZE`, `NUMPAGES`, `NUMWORDS`, and `NUMCHARS` from
  unknown fields. These fields keep their cached result text, do not claim a
  computed value, and no longer produce unsupported-field warnings. Compatibility
  note: downstream code with exhaustive matches on the public `FieldKind` enum
  must add a `DocumentInfo` arm or wildcard fallback.
- **Dynamic field diagnostics and deterministic formula/QUOTE/IF/COMPARE evaluation** —
  `FieldKind::Dynamic(String)` now distinguishes known expression, prompt, and
  merge-control fields such as `=`, `IF`, `QUOTE`, `FILLIN`, `ASK`, `COMPARE`,
  `SET`, `NEXT`, `NEXTIF`, and `SKIPIF` from unknown fields. rdoc computes
  deterministic literal arithmetic `.docx` formula fields (`=`) with numeric
  constants, literal scalar numeric/logical functions (`ABS`, `AND`, `AVERAGE`,
  `COUNT`, `DEFINED`, `FALSE`, `IF`, `INT`, `MAX`, `MIN`, `MOD`, `NOT`, `OR`,
  `PRODUCT`, `ROUND`, `SIGN`, `SUM`, `TRUE`), `+`, `-`, `*`, `/`, parentheses,
  unary signs, literal comparison operators (`=`, `<>`, `<`, `<=`, `>`, `>=`),
  simple non-spanning table-position aggregate formulas over existing plain
  numeric `LEFT`/`RIGHT`/`ABOVE`/`BELOW` cells,
  and simple `\#` numeric pictures using `0`/`#`/`x` placeholders,
  decimal places, grouping
  commas, literal prefix/suffix characters such as `$` or `%`,
  single-section leading `+`/`-` sign-control items, and `x`
  digit-drop/rounding positions, plus two- and three-section
  positive/negative/zero numeric pictures separated by semicolons,
  deterministic `.docx` `QUOTE "LiteralText"` fields with neutral
  `MERGEFORMAT`/`CHARFORMAT` and general text-format switches, plus literal
  `IF` fields for numeric comparisons and quoted string equality/inequality, and
  literal `COMPARE` fields returning `1`/`0`, including quoted `?`/`*`
  Expression2 wildcard equality/inequality; remaining dynamic/control fields,
  including bookmark/reference formula expressions, unsupported formula
  arguments, quoted text, caption references, broader numeric-picture syntax,
  and non-literal `QUOTE`/`IF`/`COMPARE` forms, preserve cached result text and
  report `NoComputedResult` instead of `UnknownField`.
  Compatibility note: downstream code with exhaustive matches on the public
  `FieldKind` enum must add a `Dynamic` arm or wildcard fallback.
- **Inserted-content field diagnostics** — `FieldKind::InsertedContent(String)`
  now distinguishes `INCLUDETEXT`, `INCLUDEPICTURE`, `LINK`, `EMBED`,
  `DATABASE`, `DDE`, `DDEAUTO`, `IMPORT`, `INCLUDE`, `AUTOTEXT`, and
  `AUTOTEXTLIST` fields from unknown fields. rdoc preserves their cached result
  text, does not evaluate linked/external content, and reports
  `NoComputedResult` instead of `UnknownField`. Compatibility note:
  downstream code with exhaustive matches on the public `FieldKind` enum must
  add an `InsertedContent` arm or wildcard fallback.
- **Mail-merge helper field diagnostics** — `FieldKind::MailMerge(String)` now
  distinguishes `ADDRESSBLOCK`, `GREETINGLINE`, `MERGEREC`, and `MERGESEQ`
  fields from unknown fields. rdoc preserves their cached result text, does not
  evaluate merge records, and reports `NoComputedResult` instead of
  `UnknownField`. Compatibility note: downstream code with exhaustive matches
  on the public `FieldKind` enum must add a `MailMerge` arm or wildcard
  fallback.
- **Reference/index field diagnostics** — `FieldKind::ReferenceIndex(String)`
  now distinguishes `BIBLIOGRAPHY`, `CITATION`, `INDEX`, `RD`, `TA`, `TOA`, and
  `XE` fields from unknown fields. rdoc preserves their cached result text, does
  not evaluate bibliography/index/table-of-authorities semantics, and reports
  `NoComputedResult` instead of `UnknownField`. Compatibility note: downstream
  code with exhaustive matches on the public `FieldKind` enum must add a
  `ReferenceIndex` arm or wildcard fallback.
- **Numbering/list field diagnostics** — `FieldKind::Numbering(String)` now
  distinguishes `AUTONUM`, `AUTONUMLGL`, `AUTONUMOUT`, `BIDIOUTLINE`, and
  `LISTNUM` fields from unknown fields. rdoc computes deterministic source-order
  plain `AUTONUM` values with common number-format switches and the documented
  `\s` separator switch, including unquoted or quoted one-character separators,
  standalone plain/neutral `AUTONUMLGL` and `AUTONUMOUT` values on the same
  counter, plus level-1 `LISTNUM NumberDefault` values with common number-format
  switches and `\s` starts/resets; the remaining contextual
  automatic-numbering/list fields preserve cached result text and report
  `NoComputedResult` instead of
  `UnknownField`. Compatibility note: downstream code with exhaustive matches on
  the public `FieldKind` enum must add a `Numbering` arm or wildcard fallback.
- **Document-structure field diagnostics** —
  `FieldKind::DocumentStructure(String)` now distinguishes `REVNUM`, `SECTION`,
  `SECTIONPAGES`, and `STYLEREF` fields from unknown fields. rdoc computes
  current `SECTION` numbers when structural section breaks are available,
  structurally bounded `SECTIONPAGES` counts when no layout inference is needed,
  and deterministic body paragraph- and character-style `STYLEREF` text by
  nearest styled paragraph/run lookup; remaining document-structure/style lookup
  cases preserve cached result text and report `NoComputedResult` instead of
  `UnknownField`.
  Compatibility note: downstream code with exhaustive matches on the public
  `FieldKind` enum must add a `DocumentStructure` arm or wildcard fallback.
- **Display/layout field diagnostics and deterministic EQ/SYMBOL evaluation** —
  `FieldKind::Display(String)` now distinguishes `ADVANCE`, `EQ`, and `SYMBOL`
  fields from unknown fields. rdoc computes deterministic `.docx` `EQ \f(n,d)`
  simple fractions as plain `n/d` text with comma or semicolon separators,
  quoted/spaced operands, and documented escaped comma/open-parenthesis/backslash
  operand characters, simple `EQ \r(radicand)` and `EQ \r(degree,radicand)`
  radicals as plain root text, and deterministic `SYMBOL` character insertion
  for decimal/hex/default ANSI codepoints, Unicode `\u`, neutral `\h`, size
  `\s`, and common Symbol/Wingdings font mappings, including simple and common
  complex fields. Layout offsets, broader equation formatting,
  Shift-JIS `SYMBOL`, and broader font-specific symbol mappings preserve cached
  result text and report `NoComputedResult` instead of `UnknownField`.
  Compatibility note: downstream code with exhaustive matches on the public
  `FieldKind` enum must add a `Display` arm or wildcard fallback.
- **Action/automation field diagnostics and display-text evaluation** —
  `FieldKind::Action(String)` now distinguishes `GOTOBUTTON`, `MACROBUTTON`,
  and `PRINT` fields from unknown fields. rdoc computes deterministic
  `GOTOBUTTON`/`MACROBUTTON` display text without executing navigation or
  macros, and renders validated `PRINT` printer-control fields as hidden output
  without executing printer/PostScript instructions; unsupported action forms
  preserve cached result text and report `NoComputedResult` instead of
  `UnknownField`. Compatibility note:
  downstream code with exhaustive matches on the public `FieldKind` enum must
  add an `Action` arm or wildcard fallback.
- **Compatibility/private field diagnostics** —
  `FieldKind::Compatibility(String)` now distinguishes `PRIVATE`, `ADDIN`,
  `DATA`, `GLOSSARY`, and `HTMLACTIVEX` fields from unknown fields. rdoc
  preserves cached text, leaves opaque payloads uninterpreted, and reports
  `NoComputedResult` instead of `UnknownField`. Compatibility note: downstream
  code with exhaustive matches on the public `FieldKind` enum must add a
  `Compatibility` arm or wildcard fallback.
- **Barcode field diagnostics** — `FieldKind::Barcode(String)` now
  distinguishes `BARCODE`, `DISPLAYBARCODE`, and `MERGEBARCODE` fields from
  unknown fields. rdoc preserves cached text, does not generate barcode images,
  and reports `NoComputedResult` instead of `UnknownField`. Compatibility note:
  downstream code with exhaustive matches on the public `FieldKind` enum must
  add a `Barcode` arm or wildcard fallback.
- **Legacy form field computed subset** — `FieldKind::FormField(String)` now
  distinguishes `FORMTEXT`, `FORMCHECKBOX`, and `FORMDROPDOWN` fields from
  unknown fields. rdoc computes deterministic `w:ffData` checkbox states and
  dropdown selected `listEntry` text for `FORMCHECKBOX`/`FORMDROPDOWN`, and
  `FORMTEXT` fields materialize explicit non-empty current results or
  empty-current `w:textInput` default text; broader protected-form behavior
  preserves cached text and reports `NoComputedResult` instead of
  `UnknownField`.
  Compatibility note: downstream code with exhaustive matches on the public
  `FieldKind` enum must add a `FormField` arm or wildcard fallback.

### Security
- **Tighter zip-bomb bound for `.docx`** (`docx/mod.rs`): each ZIP part's
  decompressed size is capped at 64 MiB (was 256 MiB) and rejected up front when
  the ZIP-declared uncompressed size exceeds it, so a ~200 KB `.docx` that inflates
  to gigabytes is refused in milliseconds with a clean error instead of burning
  ~1 GiB / ~16 s.

### Fixed
- **Paragraph-owned `.docx` section page accounting** (`docx/fields.rs`,
  `report.rs`): `PAGE`/`PAGEREF` field and diagnostic scanning now defers
  paragraph-level `w:sectPr` page breaks until the paragraph closes, so fields
  and bookmarks in the same paragraph remain on the pre-break page instead of
  seeing the next section too early.
- **Field instruction state machine** (`assemble.rs`): a field with no `0x14`
  separator (`0x13 … 0x15`) left the model assembler stuck in "instruction" mode,
  silently dropping *all* document content after it. Instruction tracking is now
  a per-field stack (visible only when every open field has passed its
  separator), which also correctly hides a nested field's result when it sits
  inside an enclosing field's instruction. Recovered substantial structure across
  the 426-file corpus (tables 17.1k → 20.5k, headings 4.5k → 4.9k, bold runs
  14.1k → 16.2k, images 618 → 657) with no new panics.
- **Inline image magic scan** (`image.rs`): the raster-signature scan returned
  the first byte offset where *any* magic matched, so a chance `FF D8 FF` triple
  in the Escher/OLE binary pre-empted a real later PNG, and the 3-byte JPEG magic
  had no marker validation — extracting tens of KB of garbage as `image/jpeg`
  when a false `FF D8 FF 85` sequence appeared before the real image. Now each
  format is scanned over the whole payload in reliability
  order (PNG → GIF → JPEG), and JPEG requires `FF D8 FF <marker ≥ 0xC0>`. Suspect
  extractions on the corpus dropped from 2 to 0.
- **CHPX FC alignment for undecodable bytes** (`assemble.rs::decode_with_fc`): an
  8-bit byte with no mapping in the document codepage decodes to U+FFFD, and the
  per-character re-encode used to size its source bytes turned that into a
  multi-byte numeric character reference (`&#65533;`), over-counting and shifting
  every following character's FC so its CHPX (bold/italic/…) was misattributed.
  The byte width is now clamped to the real 1–2 byte range with a round-trip
  error treated as a single source byte.

### Added
- **Unified `.docx` (OOXML WordprocessingML) reader.** `rdoc` now reads modern
  `.docx` in addition to legacy `.doc`, into the **same** [`DocModel`] and the
  same `text` / `to_markdown` / `to_html` / `images` surfaces.
  [`Document::open`] format-detects from the magic bytes (OLE2 `D0CF11E0` → `.doc`,
  ZIP `PK` → `.docx`). The `.docx` path (behind a default-on `docx` cargo feature
  using `zip` + `quick-xml`, mirroring the sibling `rxls` `.xlsx` setup) parses
  `word/document.xml` (paragraphs/runs with bold/italic/underline; tables with
  `gridSpan`/`vMerge` → colspan/rowspan), `word/styles.xml` (heading levels:
  `Heading N` / `제목 N`), `word/numbering.xml` (ordered vs bullet),
  `word/_rels` + `word/media` (hyperlink targets and inline images). It is
  recursion-depth-capped (no stack-overflow on pathological nesting), XXE-safe
  (external XML entities are never resolved), and zip-bomb-guarded. Disable with
  `default-features = false` for a dependency-light `.doc`-only build. The goal is
  *unification + ownership* (one Word crate, one IR, no JVM, no external `.docx`
  dependency) — not to outdo the mature `docx-rs` on `.docx` features; see the
  README. Validated against python-docx on the 127-file Apache POI `.docx` corpus:
  **98.6% mean / 100% median set-word recall, 85/87 files ≥ 99%, 0 panics.**
- **Full document model + Markdown/HTML export.** Beyond flat text, `rdoc` now
  builds a lazy typed IR (`Document::model` → `Vec<Block>`) and renders
  `Document::to_markdown` / `Document::to_html` — no other Rust crate does this
  for the legacy binary `.doc` format. Components:
  - **Character runs** (`chpx.rs`): per-run bold/italic/underline/strike/hidden
    from the CHPX bin table (`PlcfBteChpx` → CHPX FKPs).
  - **Headings** (`stsh.rs`): from the STSH style sheet (`sti` 1–9, `istdBase`
    chain) and outline level (`sprmPOutLvl`), matching English `Heading N` and
    Korean `제목 N`.
  - **Merge-aware tables** (`table.rs`): `sprmTDefTable` → real `colspan`
    (global cell-boundary set + `fMerged`) and `rowspan`
    (`fVertRestart`/`fVertMerge` by column), with a GFM/HTML-fallback exporter.
  - **Hyperlinks** from field marks (`0x13`/`0x14`/`0x15`), and **inline
    PNG/JPEG/GIF images** extracted byte-for-byte from the `Data` stream
    (`image.rs`; `Document::images()` ≈ POI `getAllPictures`).
  - Validated on private Korean-language fixtures; the fast `text()` path is
    untouched (still ~97.4% POI parity).
- Initial public release of `rdoc`.
- `extract_text(&[u8]) -> Result<String>` convenience entry point.
- `Document` API: `open`, `text`, `main_text`, `footnote_text`, `header_text`,
  `char_count`, `is_complex`.
- OLE2/CFB container access, FIB parsing via variable-length navigation
  (csw/cslw), CLX/piece-table decoding, UTF-16LE and cp1252 (`fCompressed`)
  piece decoding, Word control-mark handling, and line normalization.
- Typed [`Error`] enum; panic-free, bounds-checked parsing.
- Synthetic-`.doc` round-trip tests, `extract` example, README, and MIT license.

### Changed
- Compressed (8-bit) pieces are now decoded in the document's ANSI codepage
  derived from the FIB language id (`lid`) — Korean `0x0412` → cp949/EUC-KR,
  Japanese → cp932, etc. — instead of being hard-coded to cp1252. (Informed by
  LibreOffice ww8 `GetCharSetFromLanguage`, antiword, catdoc.)
- Control-mark handling: column break (`0x0E`) → newline, non-breaking hyphen
  (`0x1E`) → `-`, non-breaking space (`0xA0`) → regular space, and the `0x09`
  tab is now **preserved** (it is real content that POI keeps) instead of being
  dropped with other control characters.

### Added
- **Table reconstruction** (first step toward a full parser): parse the
  `PlcfBtePapx` bin table and PAPX FKP pages ([MS-DOC] 2.8.25 / 2.9.137) to
  recover each paragraph's `fInTable`/`fTtp` flags, and render table cell marks
  (`0x07`) as POI does — cells tab-separated, rows newline-separated. Degrades
  gracefully (newline per cell) when paragraph properties are unavailable.
- **List autonumber reconstruction**: parse `PlfLst`/`LSTF`/`LVL`/`LVLF` and
  `PlfLfo`/`LFO` ([MS-DOC] 2.9.150/.131/.132/.133/.149/.129), read each
  paragraph's `ilfo`/`ilvl`, and compute the rendered label (`1.`, `1.1`, `가.`,
  `(1)` …) with per-level counters, restart-on-higher-level, and the level's
  `xst` number template + `ixchFollow` separator. Number formats cover decimal,
  roman, letter, ordinal, circled, decimal-zero, and **Korean** (Ganada `가나다`,
  Chosung `ㄱㄴㄷ`, Sino-Korean `일이삼`, native counting). Generated labels go
  into `text()` only; `raw`/the sub-document accessors stay aligned with Word's
  CP space.

### Security / robustness
- Detect encrypted / XOR-obfuscated documents (`fEncrypted`/`fObfuscated`) and
  return `Error::Encrypted` instead of indexing scrambled bytes.
- Detect pre-Word-97 (Word 6/95, `nFib < 0x00C1`) and return
  `Error::UnsupportedVersion` so callers can route to a fallback extractor.

[Unreleased]: https://github.com/HyunjoJung/rdoc/commits/main
