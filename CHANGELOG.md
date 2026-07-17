# Changelog

All notable changes to `rwml` are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Adds an RAII `EditSession` for grouping existing package-preserving `.docx`
  mutations behind one explicit commit or package-exact drop/unwind rollback,
  including restoration of the pre-session touched-part state.
- Adds atomic `Document::refresh_read_view`; successful edit-session commits use
  it to reparse model, text, metadata, side tables, media, and renderer hints
  while retaining the authoritative package and touched-part evidence.
- Adds a deterministic public render-activation corpus for run paint/hidden
  text, explicit tabs and RTL tables, keep/widow pagination, equal-width
  columns, and bounded `wrapTopAndBottom` flow, with per-file provenance.
- Extends renderer validation with fixed-font all-page aHash, foreground ink
  IoU, explicit unmatched/capped page counts, configurable gates, and bounded
  page-pair raster streaming while retaining the historical page-1 aHash.

### Fixed

- Floating-shape preview coordinates now distinguish the page, page-margin text
  rectangle, and physical left/right/top/bottom margin bands; bounded
  `wrapTopAndBottom` flow also honors top/bottom-margin anchors when their visual
  bounds intersect body text.
- Opened `.docx` rendering now honors direct table-row `w:cantSplit`: default
  rows may use remaining page space, fitting protected rows move whole, and
  over-tall rows still split deterministically. Table-style inheritance and
  legacy `.doc` row-break SPRMs remain unsupported.
- Opened `.docx` table rows now honor resolved `keepNext`, `keepLines`, and
  default-on `widowControl` for direct cell paragraphs when choosing legal row
  fragments, while retaining deterministic progress for over-tall content.
- Opened `.docx` renders now resolve inherited and direct left, center, right,
  and decimal tab stops in top-level body paragraphs, including `clear`
  overrides, and preserve authored zero paragraph after-spacing instead of
  substituting the preview default gap.

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

[Unreleased]: https://github.com/HyunjoJung/rwml/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/HyunjoJung/rwml/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/HyunjoJung/rwml/releases/tag/v0.1.0
[MS-DOC]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/
