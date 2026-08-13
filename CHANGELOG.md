# Changelog

All notable changes to `rwml` are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Honors effective `w:cantSplit` on recursively flattened nested DOCX table
  rows: fitting protected rows start on a fresh page when needed, while rows
  taller than a fresh page still split for deterministic progress. Nested grid
  geometry and Word-exact pagination remain outside the preview renderer.
- Applies row-local `w:tblPrEx/w:tblCellMar` margin exceptions in place of the
  direct table margin property and before direct cell `w:tcMar`. Sparse or empty
  exceptions inherit omitted sides from the table style or schema defaults,
  including logical `start`/`end` mapping under `w:bidiVisual`, accepted-current
  Markup Compatibility selection, vertical-merge ownership, nested tables, and
  fresh conversion/reopen. Other `w:tblPrEx` properties remain unsupported.
- Applies a table style's bounded horizontal/vertical bands, first/last rows and
  columns, and four corner regions after `wholeTable` for cell margins, flat
  shading, vertical alignment, and percentage preferred width. Selection honors
  inherited Office region precedence, named or hexadecimal `w:tblLook`, row and
  cell `w:cnfStyle`, Word's 0-3 row/column band sizes, repeated header rows,
  spans, RTL corner mapping, and vertical-merge restart ownership; table, row,
  and direct-cell declarations still override the style at their established
  layers. Conditional borders and paragraph/run formatting remain outside this
  subset.
- Inherits a table style's own `w:tblCellMar` through its `w:basedOn` chain as
  the table's cell-margin default, beneath direct `w:tblPr`/`w:tcMar`
  declarations and above the schema defaults, including the style's `wholeTable`
  and bounded conditional cell-presentation regions.
- Applies a table style's `w:tblBorders` — from its own `w:tblPr` or its
  `wholeTable` region, resolved through `w:basedOn` — when the table declares no
  borders of its own, so grid-style tables no longer lose their borders.
- Fills a table's width, indentation, and alignment from its table style —
  `w:tblW`, `w:tblInd`, and table `w:jc`, resolved through `w:basedOn` and the
  `wholeTable` region — for values the table itself leaves unset.
- Applies a table style's `wholeTable` and bounded conditional `w:tcPr` cell
  margins, shading, vertical alignment, and preferred width to cells that
  declare none of their own.
- Inherits a table style's `w:tblLayout` and `w:bidiVisual` for tables that
  declare neither, tracking whether the table declared them so an explicit off
  value still wins.
- Honors a paragraph style's `w:numPr`, so paragraphs whose style declares list
  membership become list items even without their own `w:numPr`; a direct
  declaration still wins.
- Determines a paragraph's heading level from its style rather than from
  `w:outlineLvl`, which records an outline position that Word also sets on
  ordinary body paragraphs; such paragraphs are no longer reported as headings.
- Writes a paragraph's own `outline_level`, so an outline position set without a
  heading survives a write and reopen instead of being dropped.
- Honors `w:lvlOverride` on a `.docx` list instance — both `w:startOverride` and
  a replacement `w:lvl` definition — so a list that restarts or redefines a
  level numbers correctly. Levels an override does not mention keep the abstract
  definition. Legacy `.doc` already honored the equivalent `LFOLVL` overrides.
- Honors legacy `.doc` `LVLF.fNoRestart` restart thresholds, including levels
  supplied by an `LFOLVL` formatting override, so a deeper sequence continues
  across the configured levels but restarts after a more significant one.
- Honors legacy `.doc` `LVLF.fLegal` on base and `LFOLVL` replacement levels,
  rendering every current-level template placeholder as Arabic while preserving
  an original ArabicLZ zero-padded format.
- Preserves strict legacy `.doc` `sprmSFTitlePage` section state through the
  shared model and fresh `.docx` conversion/reopen for both single- and
  multi-section documents.
- Preserves complete legacy `.doc` document-grid modes and valid line pitch from
  `sprmSClm` and `sprmSDyaLinePitch`, plus nonnegative `sprmSDxtCharSpace`,
  through each section's shared model state and fresh `.docx`
  conversion/reopen. Enabled grids without their required valid line pitch are
  omitted, invalid operands leave prior source-order state intact, and a valid
  negative character-pitch delta clears an earlier representable value because
  the existing public model stores that field as unsigned.
- Preserves all six legacy `.doc` `sprmSTextFlow` section directions through
  `SectionSetup` and the final `DocSetup`, including `@`-font/non-`@` glyph
  rotation distinctions, exact fresh `.docx` text-direction output, and reopen
  parity. Invalid values leave the last valid source-order state intact.
- Preserves legacy `.doc` section page-number formats and restart values from
  `sprmSNfcPgn`, `sprmSFPgnRestart`, `sprmSPgnStart97`, and `sprmSPgnStart`
  through the shared model and fresh `.docx` conversion/reopen. Supported
  MSONFC values map exactly, valid unrepresented values use a bounded decimal
  fallback, non-counting values use the spec-permitted decimal fallback,
  invalid values preserve prior source-order state, and disabled restarts
  ignore their stored start.

## [0.1.2] - 2026-08-02

### Added

- Expands strict single-DIB metafile raster extraction to source-bearing
  `EMR_BITBLT`, `EMR_STRETCHBLT`, `META_DIBBITBLT`, and
  `META_DIBSTRETCHBLT` records when they use exact `SRCCOPY`, zero source
  origins, one-to-one full-frame geometry, canonical contiguous DIB payloads,
  and, for EMF, an identity source transform with `DIB_RGB_COLORS`. Raw and
  gzip-wrapped forms use the same bounded path. Source-less WMF forms,
  1-bit EMF source blits, scaling, cropping, mirroring, composition, and
  general vector replay remain unsupported.
- Adds package-preserving plain top-level DOCX paragraph insertion through
  `Document::insert_body_paragraph`. Its `0..=body_blocks().len()` position
  space inserts before an indexed atomic direct paragraph/table/content-control
  or before final body section properties, reuses the existing WML escaping,
  significant-whitespace, tab, line-break, Unicode, and forbidden-control
  handling, and remains transactional under `EditSession`. Synthetic retained
  trees cover prefixed/default namespaces, middle/append/blank placement,
  internal section-boundary adjacency, untouched-part byte stability,
  deterministic reopen, stale/committed/rolled back read views, node-budget
  failure, invalid positions, and opaque or cross-block structural hazards.
  Rich paragraph/block properties, numbering, fields, bookmarks, revisions,
  relationships, nested/story-part insertion, duplication, and indexed content
  replacement remain outside this bounded API.
- Adds explicit part-local cached field inventory and result editing through
  `Document::fields_in_part` and `Document::set_field_result_in_part` for the
  main body, real standard footnotes/endnotes, and correctly typed existing
  header/footer parts, including shared or unreferenced physical parts. The
  inventory follows accepted-current revision and `mc:AlternateContent`
  selection, excludes note separator boilerplate, and is the authoritative
  zero-based index for the transactional edit.
- Adds an RAII `EditSession` for grouping existing package-preserving `.docx`
  mutations behind one explicit commit or package-exact drop/unwind rollback,
  including restoration of the pre-session touched-part state.
- Adds atomic `Document::refresh_read_view`; successful edit-session commits use
  it to reparse model, text, metadata, side tables, media, and renderer hints
  while retaining the authoritative package and touched-part evidence.
- Adds a deterministic public render-activation corpus for run paint/hidden
  text, explicit tabs and RTL tables, keep/widow pagination, equal-width
  columns, bounded `wrapTopAndBottom` flow, and
  `table-cell-lists.docx` body/direct-cell/nested-cell numbering, bullet, and
  RTL cases, with per-file provenance.
- Extends renderer validation with fixed-font all-page aHash, foreground ink
  IoU, explicit unmatched/capped page counts, configurable gates, and bounded
  page-pair raster streaming while retaining the historical page-1 aHash.

### Changed

- Replaces the stale `~0.93 mean text recall` figure with the measured public-corpus
  result — 0.996 mean text recall and a 1.00 mean page-count ratio, with 23 of 24
  documents at exactly 1.00 — and documents why the remaining right-to-left fixture
  scores lower: complex-script clusters can only be mapped to text through PDF
  `ActualText`, which Acrobat and Chrome honor but MuPDF/PyMuPDF, pdfminer.six, and
  pypdf ignore. Rendering is unaffected.

- States the roadmap's two conventions explicitly: documented behavior is
  implemented and covered by tests, and the named limits of each bounded slice
  describe the current build rather than scope that has been declined.
  Unchecked roadmap entries are stated as open projects that stay unchecked
  until evidence closes them. Renderer parity wording no longer presents the
  remaining pagination, floating-layout, computed-field, and visual-fidelity
  gaps as inherent to the design. No behavior change.

### Fixed

- Honors a bullet level's declared `w:lvlText` glyph instead of discarding it and
  letting the renderer guess by depth, so a document that asks for a specific
  bullet gets it; levels that declare none keep the existing fallback and a
  `%N` pattern is still treated as autonumber syntax.

- Opened `.docx` PDF previews now resolve default and explicit logical-start tab
  stops in right/start-aligned RTL top-level body paragraphs from the right page
  text margin's leading edge. The stops retain their margin coordinates under
  resolved physical or logical left/right indents; deterministic fixed-font
  geometry covers default and explicit stops, multiple fields, segmented paint
  and source ranges, bounded line reflow, and the opened-DOCX path. RTL
  center/end/decimal stops remain outside this bounded path.
- Opened `.docx` PDF previews now reserve line width for resolved custom and
  default tab advances before breaking, so supported top-level LTR left/start
  and RTL right/start body content that no longer fits after a tab moves to the
  next line instead of running past the paragraph box. The reservation only
  tightens and is bounded to three passes, so line composition stays
  deterministic, and emitted page counts follow the resulting line count.
  Alignments and directions outside the resolved-tab support keep parley's own
  breaking unchanged. Post-tab field containment, Word-exact custom-tab-aware
  reflow, and Word-exact tab-driven pagination remain unsupported.
- Opened `.docx` PDF previews now keep resolved left, center, right, and
  decimal custom tab stops in page-text-margin coordinates under supported
  left, positive first-line, and hanging indents in left/start-aligned LTR
  top-level body paragraphs. Default-tab fallback targets use the same
  margin-anchored grid and are clamped to the active paragraph box. Synthetic
  fixed-font geometry covers final field placement at resolved stops, all four
  supported tab alignments, continuation-line hanging indents, and
  exact-edge/out-of-box fallback. Separate opened-DOCX evidence covers parsed
  first-line/hanging values, private tab-sidecar activation, deterministic
  layout/PDF output, and unchanged page counts for the bounded fixtures.
  Resolved LTR tab stops in ordinary and recursively nested table-cell
  paragraphs now use the same bounded path through source-aligned sidecars.
  Table-cell RTL center/end/decimal stops, center/right/justified LTR paragraph
  alignment, leaders and bar tabs, settings-defined default-tab intervals,
  implicit hanging-indent/list-marker tabs, post-tab field containment,
  custom-tab-aware line reflow, and Word-exact text-ruler behavior remain
  outside this bounded correction.
- PDF table cells now apply finite positive model-backed paragraph before/after
  spacing on emitted paragraphs to row measurement, row splitting, vertical
  alignment, repeated headers, and `LayoutPages` block and modeled `PAGE`-field
  records. Paragraph edges remain source-ordered across direct and recursively
  flattened nested cells, split rows attach before spacing only to the first
  fragment and after spacing only to the final fragment, and bounded line
  truncation does not invent a trailing edge. Unset, zero, negative, and
  non-finite values add no space. Table blocks now report the page of their first
  actually placed row instead of the pre-placement page. Synthetic direct-DOCX
  and style-inherited legacy-DOC
  fixtures cover deterministic layout and PDF output. Word-exact adjacent
  spacing collapse, line-unit/automatic/contextual spacing, spacing-only empty or
  hidden-only paragraphs, nested-grid geometry, table-cell images, and exact
  split-row vertical alignment remain outside this bounded preview behavior.
- PDF previews now paint reader-captured or deterministic fallback list markers
  in ordinary and recursively flattened table cells. One per-story fallback
  state spans surrounding body, direct-cell, nested-cell, and later body
  paragraphs; visual RTL table placement does not reverse it. Empty or
  hidden-only items retain their marker, and split rows or repeated headers
  reuse already-shaped markers without recounting. List-instance identity,
  source restart metadata, marker fonts/glyphs/tabs/alignment, nested-grid
  layout, and Word-exact RTL list typography remain outside this bounded
  support.
- Opened legacy `.doc` paragraphs now preserve direct and paragraph-style
  `sprmPShd80` palette and `sprmPShd` COLORREF shading when the source result
  collapses exactly to one explicit RGB fill. Clear shading uses its explicit
  background, solid shading uses its explicit foreground, and another
  supported pattern is retained only when both explicit colors are identical.
  Style-local values resolve through the bounded base chain before final
  direct-PAPX precedence. Repeated modifiers apply in source order; later
  structurally complete automatic, nil, patterned, invalid, or wrong-sized
  shading suppresses inherited or stale positive state, while a later valid
  modifier recovers. A truncated or unsizeable direct shading modifier
  suppresses and stops that PAPX scan; a structurally malformed style UPX
  invalidates its local style payload. Supported fills flow through the shared
  model, `.docx` conversion/reopen, and existing PDF paragraph paint without
  changing `LayoutPages`. Pattern fidelity, theme/automatic/nil distinction,
  document-default and table/list-style conditional shading, original legacy
  style-graph preservation through `.docx` conversion, paragraph borders,
  table-style effects, and piece `Pcd.Prm` paragraph properties remain
  unsupported.
- Opened legacy `.doc` tables now recover coherent positive physical top,
  left, bottom, right, inside-horizontal, and inside-vertical border channels
  from complete `sprmTDefTable` `TC80` records and direct row-mark
  `sprmTTableBorders80`/`sprmTTableBorders` operands. Compatible and modern
  direct properties following the definition apply in source order only while
  embedded cell edges remain default/inheritable. Visual RTL maps logical outer
  edges onto their physical sides, and recovered colors, widths, and supported
  line styles survive shared-model use and `.docx` conversion/reopen; PDF
  preview consumes the colors and widths through its existing solid-stroke
  paint without changing `LayoutPages`. Projection is limited to strict
  rectangular, unmerged tables with at least two rows and columns and six
  mutually coherent positive roles. Valid definitions with fewer or excess
  complete `TC80` records or equal boundaries still preserve table structure,
  while malformed operands, incomplete or zero-width grids, mixed
  automatic/explicit colors, nil/no-border roles, unsupported line effects,
  conflicting shared edges, pre-definition row borders, topology modifiers,
  and per-cell overrides retain the existing border fallback. Arbitrary
  per-cell conflict resolution, merged/ragged/nested, table-style-derived,
  piece-`Pcd.Prm`, PDF non-solid style paint, and Word-exact border rendering
  remain unsupported.
- DOCX table-formula operand scanning now ignores paragraph-property subtrees,
  so custom `w:pPr/w:tabs/w:tab` definitions no longer become visible cell
  tabs that block cached-result promotion and dependent formula evaluation.
  Genuine run-content markers, formula grammar, and tab-position layout retain
  their existing behavior.
- DOCX cached complex-field inventory now collects visible text and supported
  inline markers only while the paragraph-spanning result scan is inside run
  content. Custom paragraph tab-stop definitions under `w:pPr/w:tabs` therefore
  no longer appear as literal leading tabs in `Field::result` or
  `fields_in_part`, while genuine run tabs, breaks, hyphens, symbols, nested
  fields, accepted-current revisions, selected `mc:AlternateContent` branches,
  field order, and computed results retain their existing behavior across the
  already-supported body, real-note, and modeled header/footer stories. This
  does not compute tab positions, add namespace processing, or change
  field-family source-text reconstruction.
- DOCX `fill_content_control_by_tag`, `fill_content_controls_by_tag`, and
  `fill_template_fields` now encode input tabs/newlines as WordprocessingML
  `w:tab`/text-wrapping `w:br` run content in every already-supported body,
  real-note, and referenced header/footer location. Marker-to-plain refills
  remove stale text-wrapping markers, marker-only values written by these APIs
  retain an empty `w:t` anchor across save/reopen, alternate WML prefixes remain
  namespace-valid, and page/column breaks or foreign same-local-name elements
  are preserved. Exact marker-fragment node budgets, edge-whitespace attribute
  limits, overlap checks, and clone-and-swap rollback remain atomic. This does
  not create paragraphs, redistribute rich runs, synchronize data bindings,
  target source controls/results without any writable `w:t`, or expand the
  supported story set.
- Package-preserving `.docx` body text, cached field-result, content-control,
  and template-fill edits now traverse only the first direct
  `mc:Choice`/`mc:Fallback` branch of each `mc:AlternateContent`, including
  nested containers and namespace aliases. Untaken branches and malformed
  containers without a branch remain untouched; body field edits reject a
  reader/editor branch-inventory mismatch before mutation. Referenced
  header/footer and real-note template fills follow the same policy while
  excluding separator boilerplate. The explicit `replace_text_in_part` escape
  hatch retains its all-descendant behavior. This deliberately does not evaluate
  `Requires`, preprocess or delete alternate branches, or introduce a
  package-global writable field index.
- PDF rendering now honors model-backed whole-degree clockwise rotation for
  top-level and run-attached body raster images. Direct-model angles normalize
  modulo 360, images rotate around their centers, and finite axis-aligned
  rotated bounds drive proportional active-column/page-height fitting and
  pagination. Quarter turns use exact bounds, arbitrary angles retain
  deterministic output, and missing or undecodable image behavior is unchanged.
  Source-authored display extents, crop/flip/effects, floating-anchor offsets,
  exclusion-zone reflow, table-cell images, and Word-exact inline baseline
  placement remain unsupported.
- Opened legacy `.doc` pieces now retain their raw PCD `Prm` plus ordered CLX
  PRCs and apply bounded character formatting after CHPX. Literal `Prm0`
  values force bold, italic, strike, small caps, caps, and hidden text off or
  on. Precompiled `Prm1` groups additionally support literal underline and RTL,
  highlight palette values, and baseline/superscript/subscript, with supported
  values applied in source order and explicit clears preserved; underline
  styles collapse to the shared boolean model. Signed PRC lengths, payload
  bounds, and the 15-bit addressable count are validated, each group is scanned
  once during open, and complete unrelated property operands are skipped. The
  modifier remains aligned per emitted UTF-16 unit across compressed and
  uncompressed pieces, surrogate pairs, overlapping FC ranges, and every
  modeled story region; effective properties drive run boundaries and survive
  `.docx` conversion/reopen. Missing indices and malformed, style/reset-
  dependent, or style-relative groups atomically leave the CHPX-derived result
  unchanged. Piece-level font/size/color and complex-script effects,
  pictures/OLE, paragraph/list/table/section properties, tab changes,
  revision-original formatting, and full character-style resolution remain
  unsupported.
- Opened legacy `.doc` paragraphs now preserve valid unsigned-twip
  `sprmPDyaBefore`/`sprmPDyaAfter` spacing and positive proportional
  `sprmPDyaLine` LSPD values from paragraph styles and direct PAPX. Sparse
  style values resolve through the cycle/depth-bounded STSH base chain before
  final direct overrides; omitted values materialize the MS-DOC zero-before,
  zero-after, and single-spacing defaults through the shared model, `.docx`
  conversion/reopen, and top-level or table-cell PDF preview layout.
  Repeated values apply in source order, invalid values do not replace the last
  valid value, `sprmPIstd` discards earlier direct spacing, structurally
  malformed styles fall back atomically, and truncated direct modifiers retain
  their valid prefix.
  At-least/exact and explicit zero proportional LSPD values clear inherited
  multipliers but remain unset because the shared model cannot express those
  line rules. Line-unit, auto, and contextual spacing; Word-exact adjacent
  spacing; table/list-style effects; piece `Pcd.Prm` paragraph modifiers; and
  Data-stream indirection remain unsupported.
- Opened legacy `.doc` sections now preserve valid SED/SEPX page width, height,
  portrait/landscape orientation, nonnegative left/right/top/bottom margins,
  equal-width `sprmSCcolumns` counts from 1 through 44, and new/even/odd
  `sprmSBkc` break kinds through the shared model and `.docx`
  conversion/reopen. Valid `PlcfSed` boundaries create sections independently
  of header stories, and single-section properties reach the final `DocSetup`.
  Equal spacing follows the MS-DOC default and strict Bool8 overrides; an
  explicit unequal selector suppresses the modeled count, a later valid equal
  selector restores the last valid count, and invalid values leave prior state
  intact. Strict signed `fcSepx`/`cb` bounds and complete SPRM walking isolate
  malformed local payloads to the deterministic default while valid neighboring
  sections survive. Recovered equal-width columns reach the existing bounded PDF
  and `LayoutPages` flow. Preview pagination also starts modeled even/odd
  sections on the requested 1-based physical parity, inserting one body-empty
  filler when needed; section display-number restarts/formats do not affect that
  calculation. Continuous/new-column section marks normalize to the next-page
  fallback. Custom or unequal column widths/gaps, separator lines, manual column
  breaks, RTL column ordering, gutters/facing pages, header/footer distances,
  page borders/grids, vertical justification, negative fixed-position
  top/bottom semantics, mixed section-local render geometry, section-relative
  odd/even running-surface selection, and Word-exact filler surfaces remain
  unsupported.
- Opened `.docx` tables now preserve a complete positive direct `w:tblGrid` as
  normalized `Table::col_widths_pct` when its column count matches the
  reconstructed cell/span grid. Revision-history grids, omitted/zero/invalid
  widths, excessive columns, and count mismatches retain the existing
  content-sized fallback without panicking. Fresh `.docx` output now serializes
  valid matching model proportions as deterministic positive `w:gridCol` twip
  widths and retains the equal-grid fallback for hostile or incomplete model
  values. The existing PDF path therefore honors opened-DOCX grid proportions,
  including visual RTL mirroring and column spans. Word-exact fixed/autofit
  layout, preferred-cell/table/grid conflict resolution, `gridBefore`/
  `gridAfter`, style and row-exception widths, and absolute page-aware width
  synthesis remain unsupported.
- PDF rendering now applies finite positive preferred-percentage table widths
  within the active page or section column, maps logical leading/center/trailing
  table placement through visual RTL, bounds non-negative leading indentation
  to the remaining horizontal space, and mirrors logical cells inside the local
  table box. Repeated headers and split-row fragments retain the same outer
  coordinates; malformed table or column percentages keep finite deterministic
  fallbacks. Absolute/auto widths, true fixed/autofit layout, style and row
  exceptions, floating or nested-grid placement, negative outdents, table
  `both` justification, and legacy `.doc` outer table geometry remain outside
  this preview-grade bridge.
- PDF table grids now honor six-way model-backed solid border colors and
  positive eighth-point widths. Physical top, left, bottom, right,
  inside-horizontal, and inside-vertical channels resolve side-specific values
  before uniform and black/0.4-point fallbacks. Line widths are capped at the
  ECMA-376 maximum of 96 eighths (12 points), then conservatively clamped
  table-wide to half the smallest laid-out cell dimension. Centered rectangles
  share one visible-width strip across unequal neighboring cells and rows,
  physical sides survive visual RTL placement, modeled row/column spans
  suppress covered inside edges, and split row fragments omit artificial
  horizontal seams. Repeated headers retain paint and edge identity while
  `LayoutPages` remains unchanged. Non-solid/none styles, cell borders, theme
  and table-style inheritance, cell spacing and border-conflict resolution,
  ragged-row repair, nested-grid borders, and legacy `.doc` per-cell,
  no-border, conflicting, merged/ragged, nested, or style-derived border
  recovery remain unsupported.
- Opened legacy `.doc` paragraphs now preserve strict modern logical
  `sprmPDxaLeft`, `sprmPDxaRight`, and `sprmPDxaLeft1` signed-twip indents from
  direct PAPX and paragraph styles. Sparse style values resolve through the
  existing cycle/depth-bounded STSH base chain before final direct overrides.
  Logical leading/trailing edges resolve against final paragraph direction,
  positive first-line offsets remain first-line indents, and negative offsets
  become hanging indents through the shared model and `.docx`
  conversion/reopen. Direct `sprmPNest` is additive when a style-derived or
  direct logical-left base exists; prohibited style nesting and direct
  nest-only values without a base remain unmaterialized. Invalid XAS values do
  not replace the last valid local value, structurally malformed styles fall
  back safely, and truncated direct modifiers retain the valid prefix.
  List-level, compatibility-era, character-unit, and mirrored indents, plus
  negative-indent PDF outdenting, remain unsupported.
- Opened legacy `.doc` tables now preserve strict direct `sprmTFBiDi` and
  compatibility `sprmTFBiDi90` Bool16 direction from row-terminating PAPX.
  Repeated values of each property apply in source order, either final
  property enables visual RTL, equivalent rows remain one table, and a
  direction change starts a separate table before merge/width resolution.
  Cells stay in source-logical order through the shared model and `.docx`
  conversion while the existing PDF renderer mirrors their visual positions.
  Invalid Bool16 values are ignored and truncated modifiers retain only the
  valid prefix. Table-style/Data-stream/`Pcd.Prm` table direction, additional
  position/wrapping/protection boundaries, and nested legacy tables remain
  unsupported.
- Opened legacy `.doc` paragraphs now preserve valid direct `sprmPFBiDi`
  Bool8 direction plus physical left/center/right and logical
  start/center/end alignment from `sprmPJc80` and `sprmPJc` through the shared
  model and `.docx` conversion/reopen. Supported distribution, Kashida, and
  Thai values collapse to generic justify; the indented logical value remains
  outside the shared alignment model. Logical start/end resolve against
  paragraph direction, and paragraphs without explicit justification use that
  logical start edge. Generated BiDi paragraphs retain explicit physical-left
  alignment when present. The same bounded direction/justification subset now
  resolves through cycle/depth-bounded paragraph-style STSH inheritance before
  final direct PAPX overrides. Character-style/language-derived direction,
  list-level and compatibility-era/character-unit/mirrored legacy indents,
  exact RTL list-level layout, table-style-derived visual RTL, piece `Pcd.Prm`
  paragraph direction/modifiers,
  and Markdown/HTML visual RTL remain unsupported.
- Opened legacy `.doc` CHPX runs now preserve literal direct `sprmCFBiDi`
  on/off values through the shared model, `.docx` conversion, and PDF run
  isolation. Style-relative operands use the conservative unknown fallback,
  while character-style/reset operators preserve established direction as
  required by MS-DOC. Character-style/language-derived direction,
  complex-script properties, table-style-derived visual RTL, style-relative or
  style/reset-dependent piece `Pcd.Prm` direction modifiers,
  and Markdown/HTML visual RTL remain unsupported.
- Opened legacy `.doc` CHPX runs now preserve literal direct
  `sprmCFSmallCaps` and `sprmCFCaps` on/off values through the shared model,
  `.docx` conversion, and PDF rendering. Style-relative toggle operands and
  complete character-style/reset modifiers conservatively discard stale
  direct state until a later literal value; character-style-derived
  capitalization, style-relative `Pcd.Prm` operands, and Markdown/HTML visual
  caps/small-caps remain unsupported.
- Opened legacy `.doc` CHPX runs now preserve direct `sprmCIss` normal,
  superscript, and subscript values through the shared model, `.docx`
  conversion, and PDF rendering. Complete character-style/reset modifiers
  conservatively discard stale direct state and use the baseline fallback until
  a later valid direct value; character-style-derived alignment,
  style/reset-dependent piece `Pcd.Prm` vertical-alignment modifiers,
  arbitrary `sprmCHpsPos` baseline shifts, and Markdown/HTML visual
  super/subscript remain unsupported.
- Opened legacy `.doc` CHPX runs now preserve `sprmCHighlight` Ico palette
  values through the shared model, `.docx` conversion, and PDF rendering,
  including explicit highlight clearing and deterministic rejection of invalid
  or truncated operands. Style/reset-dependent piece `Pcd.Prm` highlighting,
  full legacy character-style resolution, and Markdown/HTML visual
  highlighting remain unsupported.
- Opened legacy `.doc` tables now preserve relative column proportions from
  coherent `sprmTDefTable` `rgdxaCenter` row boundaries, including mixed
  internal row grids represented through the existing global column spans.
  Missing, zero-width, descending, or inconsistent outer-edge geometry keeps
  the deterministic content-sized fallback; absolute table sizing, autofit,
  indentation, preferred cell widths, table-style-derived RTL, and nested
  legacy tables remain unsupported.
- Legacy `.doc` list labels now honor bounded `PlfLfo`/`LFOData`/`LFOLVL`
  per-instance start and replacement-format overrides and continue numbering
  across `ilfo` instances that share an `lsid`; malformed variable records retain
  deterministic fixed-LFO fallback without panicking.
- Opened `.docx` tables now materialize direct `dxa`/`nil`
  `w:tblCellMar` defaults and per-side direct `w:tcMar` exceptions into
  physical cell margins, including logical `start`/`end` mapping under
  `w:bidiVisual` and the `0/115/0/115`-twip schema fallback. Malformed,
  percentage, and automatic declarations inherit from the lower layer;
  table-style, conditional-style, and `w:tblPrEx` margin inheritance remain
  unsupported.
- Floating-shape preview coordinates now distinguish the page, page-margin text
  rectangle, and physical left/right/top/bottom margin bands; bounded
  `wrapTopAndBottom` flow also honors top/bottom-margin anchors when their visual
  bounds intersect body text.
- Opened `.docx` rendering now honors effective table-row `w:cantSplit` from
  direct row properties and inherited table-style chains, including bounded
  `wholeTable`, horizontal `band1Horz`/`band2Horz`, `firstRow`, and `lastRow`
  conditional regions selected by direct table `w:tblLook` or row `w:cnfStyle`.
  Horizontal bands honor inherited style and direct-table
  `w:tblStyleRowBandSize` values in Word's 0-3 range. Default rows may use
  remaining page space, fitting protected rows move whole, and over-tall rows
  still split deterministically. Vertical/column/corner and `w:tblPrEx` cases
  remain unsupported.
- Opened legacy `.doc` rendering now honors direct table-row
  `sprmTFCantSplit` and compatibility `sprmTFCantSplit90`: rows remain
  splittable by default, fitting protected rows move whole, the modern property
  takes precedence when both are present, and over-tall rows still split for
  deterministic progress. Inherited/table-style and nested legacy row controls
  remain unsupported.
- Opened legacy `.doc` rendering now resolves paragraph-style STSH inheritance
  followed by direct PAPX for `sprmPFKeep`, `sprmPFKeepFollow`, default-on
  `sprmPFWidowControl`, and `sprmPFPageBreakBefore` on emitted nonblank
  top-level and ordinary table-cell paragraphs. Private source-aligned hints
  carry keep/widow behavior, the existing model property carries page breaks,
  and explicit direct-off values override inherited-on values. Over-tall
  content still splits deterministically. Other STSH properties,
  table/list-style effects, piece `Pcd.Prm` paragraph modifiers, nested legacy
  tables, and controls on discarded blank top-level paragraphs remain
  unsupported.
- Opened `.docx` table rows now honor resolved `keepNext`, `keepLines`, and
  default-on `widowControl` for direct, accepted-current wrapper-contained, and
  recursively nested cell paragraphs when choosing legal row fragments, while
  isolating paragraph chains by cell and retaining deterministic progress for
  over-tall content. Nested content remains a bounded flattened-text preview,
  not nested grid layout.
- Opened `.docx` renders now resolve inherited and direct left, center, right,
  and decimal tab stops in top-level body paragraphs, including `clear`
  overrides, and preserve authored zero paragraph after-spacing instead of
  substituting the preview default gap.
- Opened `.docx` paragraphs now resolve document defaults, the declared default
  paragraph style for unstyled paragraphs, and bounded explicit paragraph-style
  chains for before/after spacing, proportional automatic line spacing,
  first-line/hanging indents, flat RGB shading, and
  `pageBreakBefore`, with direct zero/off precedence. Unsupported nearer
  exact/at-least line-rule, nonzero line-unit spacing, nonzero character-unit
  indent, enabled automatic spacing, theme, pattern, nil, or malformed forms
  suppress representable inherited values instead of leaking base formatting;
  paragraph-style chain limits are evaluated independently of map iteration
  order. Zero line- and character-unit attributes clear related inherited unit
  overrides; nonzero unit values remain outside the represented subset.

## [0.1.1] - 2026-07-16

Patch-compatible stabilization release. Default-feature and all-feature public
APIs are checked against `v0.1.0` by `cargo-semver-checks`.

### Added

- Expands bounded WMF/EMF single-DIB raster extraction to 1/4/8-bit palettes,
  16/32-bit `BI_BITFIELDS`, and full-scan SETDIB records, with strict identity
  transfer semantics and decoded-allocation/inflation ceilings.
- Improves preview PDF fidelity for modeled run paint, vertical alignment, and
  hidden-content exclusion; paragraph shading, spacing, indents, and tabs; table
  cell margins and RTL visual order; bounded paragraph/run RTL shaping with
  bundled Arabic/Hebrew subsets; and equal-width section-column flow.
- Applies resolved `.docx` `keepNext`, `keepLines`, and default-on
  `widowControl` to opened-document PDF/layout pagination through private
  source-aligned hints; raw `DocModel` render entry points remain source-agnostic.
- Applies bounded forward `wrapTopAndBottom` exclusion after eligible recovered
  top-level paragraph anchors with explicit page/margin or enabled `simplePos`
  vertical geometry, while retaining overlay fallback for unsupported wrap cases.
- Adds typed enumeration plus package-preserving move/removal for conservative
  atomic direct `.docx` body paragraphs, tables, and content-control subtrees,
  with transactional cross-block range/field/section/opaque-content rejection.
- Computes side-table `STYLEREF` fields in headers, footers, footnotes,
  endnotes, comments, and text boxes with report/evaluator parity.
- Adds a license-clean public legacy `.doc` extraction corpus with exact report
  expectations and Apache POI 5.2.3 / LibreOffice 26.2.3.2 goldens.
- Executes generated WASM bindings under Node in CI and release preflight, and
  freezes document/render report JSON shapes with checked-in golden contracts.
- Adds release-mode public-corpus performance, bundled-font, fuzz-build,
  deterministic-corpus, package-identity, and semantic-version gates. Release
  automation publishes the checksum-verified `rwml-fonts` dependency first,
  waits for registry visibility, and then packages and publishes `rwml` with
  idempotent registry-identity checks.

### Fixed

- Tightens single-DIB WMF/EMF extraction to spec-correct inclusive EMF bounds,
  consistent headers and terminal records, frame-covering destinations, and an
  exact one-raster record stream so later vector composition stays unsupported.
- Preservation edits now resolve targets across accepted revisions, ignore
  deleted comment markers and nested tables, handle rejected header revisions,
  retain comment run formatting, and validate mutations before commit.
- Fixes 32-bit WASM compilation without changing the public `usize` APIs.

### Security

- Expands the edit fuzz target across the package-preserving mutation surface,
  makes its lockfile reproducible, and seeds parse/edit/render fuzzing from the
  public hostile-input corpus.

## [0.1.0] - 2026-07-04

First public release. `rwml` is a native Rust toolkit for Microsoft Word
documents — **read, write, edit, and render** — covering **both** legacy `.doc`
(Word 97–2003 OLE2 binary, [MS-DOC]) and modern `.docx` (OOXML WordprocessingML),
with no JVM, no Apache POI, no other `.docx` crate, and no subprocess.
`#![forbid(unsafe_code)]`, fuzzed, XXE-safe, and zip-bomb-guarded.

### Added

#### Formats & model
- **One model for both formats.** [`Document::open`] format-detects from the
  magic bytes (OLE2 `D0CF11E0` → `.doc`, ZIP `PK` → `.docx`) and **both** backends
  produce the same [`DocModel`], so the Markdown/HTML exporters, the `.docx`
  writer, and the PDF renderer all consume one IR and callers never branch on the
  source format.
- **Typed IR + Markdown/HTML export.** `Document::model` → `Vec<Block>`
  (`Paragraph | Table | Image | PageBreak | SectionBreak`) with lazy typed
  construction, plus `to_markdown` / `to_html` — no other Rust crate does this for
  the legacy binary `.doc` format. `CharProps` carries font/size/color/highlight/
  vert-align/small-caps; `ParaProps` gains spacing/indent/shading and `bidi`;
  `Cell`/`Table` gain shading, vertical alignment, and column widths; new
  `DocSetup`/`PageSetup`. All additive and `Default`.
- `extract_text(&[u8]) -> Result<String>` convenience entry point; `Document`
  API `open`, `text`, `main_text`, `footnote_text`, `header_text`, `char_count`,
  `is_complex`; typed [`Error`] enum with panic-free, bounds-checked parsing.

#### Reading
- **Unified `.docx` (OOXML WordprocessingML) reader.** Behind the default-on
  `docx` feature (`zip` + `quick-xml`), parses `word/document.xml`
  (paragraphs/runs with bold/italic/underline; tables with `gridSpan`/`vMerge` →
  colspan/rowspan), `word/styles.xml` (heading levels: `Heading N` / `제목 N`),
  `word/numbering.xml` (ordered vs bullet, per-level labels), `word/_rels` +
  `word/media` (hyperlink targets and inline images), comments, tracked
  revisions, footnotes/endnotes, and text boxes. Recursion-depth-capped, XXE-safe,
  and zip-bomb-guarded. Validated against python-docx on the 127-file Apache POI
  `.docx` corpus: **98.6% mean / 100% median set-word recall, 85/87 files ≥ 99%,
  0 panics.** Disable with `default-features = false` for a dependency-light
  `.doc`-only build.
- **Legacy `.doc` reader.** OLE2 compound-file access via `cfb`, FIB parsing by
  navigating variable-length sub-structures (never hard-coded offsets), CLX/
  piece-table decoding, UTF-16LE and codepage-aware 8-bit (`fCompressed`) piece
  decoding in the document's ANSI codepage derived from the FIB language id
  (`lid`) — Korean `0x0412` → cp949/EUC-KR, Japanese → cp932, etc. Rich second
  pass: CHPX character-property bins (bold/italic/underline/strike/hidden, font
  name from `SttbfFfn`, half-point size, color), STSH stylesheet + outline levels
  (headings, English `Heading N` and Korean `제목 N`), `sprmTDefTable`
  merge-aware tables (real colspan/rowspan), `PlfLst`/`LSTF`/`LVL` list
  autonumbers (decimal, roman, letter, ordinal, circled, and Korean `가나다`/
  `ㄱㄴㄷ`/`일이삼`/native counting), hyperlink field marks, and `PICF` inline
  PNG/JPEG/GIF images (`images()` ≈ POI `getAllPictures`). Control-mark handling
  matches POI (tab preserved; column break → newline; non-breaking hyphen/space
  normalized).
- **Per-section legacy `.doc` headers/footers.** `PlcfSed` section boundaries
  flow into the shared model as section breaks, so each legacy section's
  `PlcfHdd` story group (default/even/first header and footer variants) applies to
  its own section; `HeaderFooter` gains a public `section: Option<usize>` field.
  Single-section and malformed-table documents keep prior behavior.
- **Exact legacy `.doc` note and shape anchors.** Footnotes/endnotes anchor at
  their `PlcffndRef`/`PlcfendRef` reference positions (every note), and text boxes
  anchor at their `PlcSpaMom` `SPA` positions when counts align; a single
  unambiguous marker anchors to its containing body text, and malformed tables
  keep source-region anchors.
- **Legacy `.doc` comment author metadata.** Legacy comments carry `author` and
  `initials` recovered from the `PlcfandRef` `ATRDPre10` records and the
  `GrpXstAtnOwners` owner-name table; truncated tables leave the fields unset.
- **Comment metadata.** `Comment` gains a public `resolved: Option<bool>`
  recovered from `commentsExtended.xml` (`w15:done`), distinguishing resolved
  from open comments.
- **Style-inheritance-resolved run formatting** and richer read model surfaces
  across both backends (only direct `w:rPr` is read for `.docx`, matching `.doc`
  CHPX behavior).

#### Fields
- **Deterministic field evaluators**, source-order stable, spanning the field
  families: formula/expression (`=`, `IF`, `QUOTE`, `COMPARE`, `FILLIN`, `ASK`,
  `SET`, `NEXT`/`NEXTIF`/`SKIPIF`), table-position aggregate formulas, `PAGE`/
  `PAGEREF`, `REF`, `STYLEREF`, `TOC`/`TC`/`SEQ`, `NOTEREF`/`FTNREF`, document-
  info/date/stat (`DATE`, `TIME`, `AUTHOR`, `TITLE`, `NUMPAGES`, `FILESIZE`, …),
  `SECTION`/`SECTIONPAGES`/`REVNUM`, display/layout (`EQ`, `SYMBOL`, `ADVANCE`),
  numbering/list (`AUTONUM`/`AUTONUMLGL`/`AUTONUMOUT`/`LISTNUM`), legacy form
  (`FORMTEXT`/`FORMCHECKBOX`/`FORMDROPDOWN`), and diagnostic-only families
  (inserted/external content, mail-merge helpers, reference/index, action/
  automation, compatibility/private, barcode). Each `FieldKind` is distinguished
  from unknown fields; unsupported fields preserve cached display text and report
  a precise reason (`UnknownField` / `UnresolvedBookmark` / `UnsupportedSwitch` /
  `NoComputedResult`) with both field-kind counts and reason counts in
  diagnostics.
- **Caller-supplied field evaluation context.** `FieldContext` +
  `Document::fields_with_context` compute volatile fields deterministically from
  caller inputs: `DATE`/`TIME` with an explicit `\@` picture format the supplied
  timestamp, and `USERNAME`/`USERINITIALS`/`USERADDRESS` without literal overrides
  use the supplied identity. Context is an input and never overrides
  document-derived computed results; `fields()` behavior is unchanged.
- **MERGEFIELD / template-fill support** via `fill_template_fields` (content
  controls + MERGEFIELD cached results); INCLUDETEXT and other inserted/external
  content fields are named and keep cached text without evaluating linked content.
- **Side-table field evaluation.** The deterministic evaluation subset that
  applies to body text now also computes inside comment bodies and anchors,
  tracked-change (revision) text, note anchors, floating-shape and text-box text,
  and TOC heading sources, with matching document/render report parity.
- **`REF` numeric-picture and lock-result switches.** `REF <bookmark> \# "<pic>"`
  formats a numeric bookmark value through the shared number-picture formatter
  (falling back to cached text with `NoComputedResult` when the bookmark is
  non-numeric), and the `\!` lock-result switch is accepted as a neutral switch.
- **`NOTEREF` numbering fidelity.** `w:customMarkFollows` note references no
  longer consume an auto-number, so later `NOTEREF` ordinals are correct; and
  document-level `w:footnotePr`/`w:endnotePr` `numStart`/`numFmt` from
  `settings.xml` are applied to computed note numbers (per-page restart stays a
  documented layout-dependent ceiling).

#### Writing & editing
- **`.docx` authoring (`rwml::write_docx`, `DocBuilder`).** Build a `DocModel`
  from data and serialize a clean, Office-openable `.docx`: rich `w:rPr` (font,
  half-point size, color, highlight, small-caps, super/subscript), `w:pPr` (named
  heading styles via a synthesized `styles.xml` with `outlineLvl`, alignment,
  spacing, indent, shading, page-break-before), leveled lists, bordered tables
  with width/fixed-layout/alignment/indentation/per-side border widths/styles/
  colors and per-cell shading/width/margins/vertical alignment, images with alt
  text/pixel size/inline rotation/floating offsets, simple fields with cached
  results, `PAGEREF` helper runs, dirty TOC heading-range fields, run-anchored
  comments with reply parent ids and commentsExtended metadata, tracked
  insertion/deletion runs, run-level content controls with data-binding metadata,
  bookmarked runs, authored footnotes/endnotes, string custom document properties,
  raw custom XML data-store items, generated core metadata, explicit Word document
  ids, web-extension task pane package shells, page setup with section columns/
  document grids/text direction/title pages/page-number restarts, section breaks,
  and styled default/first/even running headers/footers with page numbers. All
  round-trips through the reader; opens in Word (verified via python-docx) and
  LibreOffice. `try_write_docx` is a fallible `write_docx`. See
  `examples/report.rs`.
- **Chart authoring.** `ChartBuilder` emits the current core OOXML chart families
  — bar/column/line/area (incl. stacked, 100%-stacked, and 3-D variants), radar,
  scatter, bubble, pie/doughnut (incl. exploded and 3-D), surface/3-D surface,
  high-low-close stock and stock, and pie-of-pie/bar-of-pie — with embedded
  workbook-backed source data,
  plus `wireframe()` styling for surface-family charts and `ChartShape` styling
  (cylinder/pyramid) for 3-D bar/column-family charts.
- **Package-preserving `.docx` editing (`Document::open` → edit → `save`).**
  Opening a `.docx` retains the whole OPC package, and `save() -> Result<Vec<u8>>`
  re-emits it with every unmodeled part preserved verbatim — themes, settings,
  fonts, comments, custom XML, charts, embeddings, and unknown/future parts. A
  no-op open→save is part-payload byte-stable. Editing is **element-tree only**:
  `replace_body_text`, `set_field_result`, `fill_content_controls_by_tag`,
  `fill_template_fields`, `accept_all_revisions`/`reject_all_revisions`,
  `set_hyperlink_target`, `set_comment_text`/`add_comment_on_text`,
  `replace_header_footer_text`/`replace_text_in_part`, `add_footnote_on_text`/
  `add_endnote_on_text`/`replace_note_text`, `set_table_cell_text`, image
  add/replace for PNG/JPEG/GIF/BMP/TIFF/WebP, and `set_core_property` mutate the
  live `document.xml` element tree or media parts, so fields, content controls
  (`w:sdt`), `mc:AlternateContent` shapes, comments, and tracked changes are
  preserved; lazy promotion re-serializes only the edited part. `Document::new()`
  starts from a bundled blank template. New internals: `opc` (OPC round-trip
  layer) and `xmltree` (an edit-preserving arena XML tree). Validated on the
  127-file corpus with python-docx as the strict external checker.
- **Marker fidelity.** `set_field_result`, `replace_body_text`,
  `replace_header_footer_text`, `replace_text_in_part`, `set_table_cell_text`,
  and comment/note edits write user-supplied tabs/newlines as WordprocessingML
  `w:tab`/`w:br` markers (and `xml:space="preserve"` for leading/trailing
  whitespace) instead of literal control characters.

#### Rendering (`render` feature)
- **PDF rendering (`rwml::render_pdf` / `to_pdf`).** Native typesetting with
  `parley` (Korean/CJK [UAX #14] line-breaking + script font fallback) and
  `krilla` (subsetted embedded fonts, selectable text). Honors run color/size/
  font, caps/small-caps, lists with real autonumber labels and indentation,
  bordered tables with shaded vertically-aligned cells and authored column widths,
  images (PNG/JPEG/GIF/WebP), and clickable hyperlink annotations; page size/
  orientation/per-side margins come from the model; multi-page tables repeat
  header rows and oversized rows split across pages.
  `render_pdf_with_fonts` registers caller-supplied fonts for headless/server use;
  `render_pdf_with_report` / `to_pdf_with_report` expose the emitted page count
  and renderer warnings. **Preview-grade, not a LibreOffice replacement**: exact
  pagination and exact floating-object layout differ. Measured against LibreOffice
  on a real corpus it reaches ~0.93 `.docx` text recall (~0.96 for `.doc`) with
  close page counts.
- **Layout-derived page numbers.** `layout_pages_with_fonts` /
  `Document::layout_pages_with_fonts` report the page count and the page each body
  `PAGE` field and top-level block lands on in rwml's own preview-grade pagination
  — matching rwml's PDF output, **not** Microsoft Word's pagination. Supplied
  fonts are used strictly (system fonts disabled), so identical document + font
  bytes yield identical results; values live in a separate `LayoutPages` record
  and never touch `Field::computed_result`.
- **Floating-shape overlays.** PDF previews draw recovered `.docx` `wp:anchor`
  geometry as approximate overlay boxes with `behindDoc` z-ordering, resolving the
  anchor line to a best-effort top-level body block page (through transparent
  content controls, custom XML, single-branch `mc:AlternateContent`, and
  accepted/current revision wrappers); compact placeholder lines stand in for
  preserved charts, OLE objects, and unsupported metafile images.
- **Metafile diagnostics.** Single-DIB raster extraction plus
  `report().features.metafiles` exposing WMF/EMF/EMZ/WMZ part path, format, byte
  size, compression flag, and header-derived dimensions when a raw or gzip-wrapped
  header makes that cheap to recover.
- **RTL property plumbing.** `ParaProps.bidi` (`w:bidi`), `CharProps.rtl`
  (`w:rtl`), and `Table.bidi_visual` (`w:bidiVisual`) are read from `.docx`,
  round-trip through `write_docx`, and get builder setters. Shaping and alignment
  mirroring in the renderer are a later milestone.
- **Optional `bundled-fonts` feature** pulls the separate OFL-licensed
  `rwml-fonts` companion crate and exposes `render_pdf_bundled` /
  `try_render_pdf_bundled` for KS X 1001 Hangul + hanja plus Latin PDF rendering
  without changing rwml's MIT license.

#### Diagnostics & safety
- **Feature inventory / report JSON.** `Document::report()` (and its `to_json()`)
  surfaces format, stats, edit capability/edited-part names, core + custom
  document properties, and a feature inventory (notes, text boxes, floating
  shapes, metafiles, fields with field-kind and reason counts). `edited_parts()`
  reports touched package parts.
- **Safety.** `#![forbid(unsafe_code)]`; malformed or hostile input returns an
  `Error` or read-only diagnostics, never a panic; bounds-checked parsing; fuzz
  targets (read, render, and a scripted preservation-`edit` target); XXE-safe XML
  (external entities never resolved); and a zip-bomb guard capping each `.docx`
  ZIP part's decompressed size at 64 MiB, rejected up front when the declared
  uncompressed size exceeds it.
- **Encrypted / obsolete detection.** Encrypted / XOR-obfuscated documents
  (`fEncrypted`/`fObfuscated`) return `Error::Encrypted`, and pre-Word-97 files
  (Word 6/95, `nFib < 0x00C1`) return `Error::UnsupportedVersion`, instead of
  emitting garbage.
- **Release tooling.** `scripts/render_validate.py` (recall / page-count /
  visual-hash vs LibreOffice), `scripts/bench_vs_mature.py` (extraction benchmark
  vs POI/LibreOffice goldens), `scripts/public_hygiene_audit.py`, and
  `scripts/release_manifest.py` (named `public-release` policy embedding required
  Rust gates and selected optional render/extraction thresholds).

### Removed
- **The Phase-A model-overlay edit surface** (`Document::body_mut()` /
  `apply_body_overlay`): regenerating `document.xml` from the lossy model cannot
  preserve body-coordinated constructs, so package-preserving editing is
  element-tree only.

[Unreleased]: https://github.com/HyunjoJung/rwml/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/HyunjoJung/rwml/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/HyunjoJung/rwml/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/HyunjoJung/rwml/releases/tag/v0.1.0
[MS-DOC]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/
