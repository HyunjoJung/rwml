# PRD - rdoc native Word engine

**Status:** Draft, active implementation baseline
**Updated:** 2026-06-29
**Owner:** rdoc
**Scope:** long-term product direction for a native Rust Microsoft Word document
engine, covering `.doc`, `.docx`, package-preserving editing, `.docx` authoring,
and native rendering.

## 1. Purpose

rdoc should grow from a useful reader/writer crate into a mature native document
engine for Microsoft Word formats.

The project is not only a text extractor and not only a `.docx` builder. Its
long-term value is the combination of these properties:

- one Rust API for legacy `.doc` and modern `.docx`;
- strong semantic extraction into a shared model;
- package-preserving edits that do not damage unknown OOXML content;
- first-class `.docx` authoring APIs;
- an in-process native preview renderer;
- corpus, fuzz, and differential testing that make the implementation credible.

The public positioning should be ambitious but precise:

> rdoc is a native Rust Word document engine: a unified reader for `.doc` and
> `.docx`, a preservation-oriented `.docx` editor, an authoring toolkit, and an
> experimental native PDF preview renderer.

## 2. Product Goals

### G1. Deep unified reading

rdoc should read `.doc` and `.docx` into the same semantic model without making
callers branch on file format. The model should grow beyond body text and basic
tables into the structures that real Word documents rely on.

Required direction:

- body, headers, footers, footnotes, endnotes, text boxes, tables, lists, images;
- comments and tracked changes as visible model entities;
- hyperlinks as reportable document features;
- fields as explicit structures with instruction, cached result, and supported
  computed values where feasible;
- shapes and unsupported embedded objects represented as typed placeholders;
- metafile image diagnostics that expose package path, WMF/EMF family, stored
  byte size, compression flag, and cheap raw or gzip-wrapped header dimensions
  when recoverable;
- diagnostics for content that rdoc saw but cannot yet model.

Success means callers can answer both "what text is in this document?" and "what
Word features did this document contain?" without parsing raw OOXML themselves.

### G2. Preservation-first editing

rdoc should be trusted on foreign `.docx` files. Opening, making a focused edit,
and saving must not discard unrelated parts or silently flatten the document.

Required direction:

- no-op `open -> save` keeps unedited package parts stable;
- edits operate on retained package state, not on a lossy regenerated model;
- all mutations are transactional across XML, relationships, content types, and
  media parts;
- unmodeled body content is preserved when possible and reported when not;
- errors prefer refusal over partial, corrupt output.

The editor can start small, but its contract must stay strict.

### G3. Authoring engine

rdoc should support generated Word documents well enough for production report
pipelines.

Required direction:

- ergonomic document builder APIs over `DocModel`;
- styles, numbering, sections, page setup, headers, footers, images, hyperlinks,
  tables, and rich text;
- comments, fields, tracked insertions/deletions, charts, and content controls as
  builder surfaces;
- chart authoring should cover bar, stacked bar, 100% stacked bar, 3-D bar, stacked 3-D bar, 100% stacked 3-D bar, column, stacked column, 100% stacked column, 3-D column, stacked 3-D column, 100% stacked 3-D column,
  line, markerless line, smooth line, stacked line, 100% stacked line, 3-D line, area, stacked area, 100% stacked area, 3-D area, stacked 3-D area, 100% stacked 3-D area,
  radar, radar-with-markers, filled radar, scatter, line-only scatter, smooth scatter, smooth markerless scatter, marker-only scatter, bubble, 3-D bubble, pie, exploded pie, 3-D pie, exploded 3-D pie, doughnut, exploded doughnut,
  surface, 3-D surface, high-low-close stock, stock, pie-of-pie, and bar-of-pie charts with literal chart caches plus embedded
  workbook-backed source data, plus shape styling for 3-D bar/column-family charts and
  wireframe styling for surface-family charts;
  newer extension chart families can mature separately;
- predictable output that opens in Word and LibreOffice;
- readable examples that behave like realistic reports, not toy snippets.

The authoring path may be lossy relative to a foreign source package. That is
acceptable if it is clearly separated from preservation editing.

### G4. Native preview renderer

rdoc should keep the native renderer and deepen it over time. The renderer does
not need to claim Word-exact output, but it should become measurable and useful.

Required direction:

- selectable-text PDF output;
- stable page geometry from document setup;
- text shaping, CJK fallback, lists, tables, headers, footers, images, links;
- progressive support for fields, approximate floating-object previews, RTL, and
  bundled fonts;
- render validation against external tools and golden outputs.

The public promise is "native preview/report renderer", not "Word layout clone".

### G5. Maturity infrastructure

The engine should be backed by the same kind of evidence expected from mature
document libraries.

Required direction:

- public, redistributable corpus;
- private corpus support through local scripts without committing private data;
- differential tests against mature tools where appropriate;
- fuzzing for `.doc`, `.docx`, XML, ZIP, OLE, and edit operations;
- release gates for panic-free parsing, package preservation, renderer
  regression, and extraction benchmark trend thresholds;
- clear unsupported-feature reports.

## 3. Target Users

### Rust library users

They want in-process document parsing, indexing, conversion, report generation,
or safe document mutation without a JVM, Office automation, or subprocesses.

### Search and ingestion systems

They want robust text and metadata extraction across old `.doc` and modern
`.docx` files, including malformed or hostile inputs.

### Report generation systems

They want `.docx` output from structured data with tables, headers, images,
numbering, page setup, and reliable opening in Office suites.

### Local tools and future UI surfaces

They may eventually use WASM, CLI, or desktop wrappers, but the core engine must
remain useful without those product layers.

## 4. Non-Goals and Deferred Goals

These are not rejected permanently, but they should not distort the core design.

- Word-exact layout parity as a short-term promise.
- In-place editing for legacy binary `.doc`.
- Schema-complete OOXML coverage through generated bindings.
- Office automation or a LibreOffice subprocess as the core implementation.
- UI/editor productization before the Rust core has strong diagnostics and
  preservation guarantees.

Deferred goals:

- WASM viewer;
- web demo;
- editor-like surfaces;
- plugin integrations;
- exact visual diff infrastructure;
- richer native PDF/vector replay.

## 5. Public Release Criteria

The first public maturity release should meet these requirements.

### Reader

- `.doc` and `.docx` open through one API.
- Text extraction is panic-free on the public corpus.
- Core metadata is queryable through `Document::core_properties()` for supported
  `docProps/core.xml` fields including title, subject, creator, description,
  keywords, category, content status, last-modified-by, created, modified,
  last-printed, revision, and version metadata.
- `DocModel` includes body paragraphs, runs, tables, lists, images, headers,
  footers, footnotes, endnotes, and text boxes where currently supported.
- Footnotes and endnotes are exposed through `Document::notes()` as typed
  side-table records where the reader can recover them; `.docx` records include
  Word reference ids plus normalized containing body block text for matched note
  references, including through accepted-current body-level revision wrappers.
- Text boxes are exposed through `Document::text_boxes()` as typed side-table
  records where the reader can recover them, following the accepted-current
  revision policy for `.docx` body text boxes.
- `.docx` headers and footers are exposed through `Document::header_footers()`
  as referenced part records with stable `part#type` ids and default, even-page,
  and first-page variants where present; paragraph section-break setup and final
  `DocSetup` store default, first-page, and even-page variants, including
  inherited default section references when omitted, and authored `.docx` output
  can emit those variant references. Native PDF rendering selects those running
  variants per section, with first-page variants scoped to the section start and
  even variants based on emitted page parity.
- `.docx` comments expose side-table text and best-effort body anchors including
  visible tab and break markers, with anchor text following the accepted-current
  revision policy.
- `.docx` tracked changes expose side-table insertion/deletion/move records, and
  the default accepted body model includes inline and block-level inserted or
  moved-to current content while excluding deleted or moved-from old content.
- `.docx` field records follow the same accepted-current revision policy,
  including direct, inserted, and moved-to fields while omitting deleted and
  moved-from old fields.
- Legacy `.doc` annotation, note, and text-box side-table records expose
  best-effort source-region anchors when exact body or shape anchors are not yet
  decoded.
- Legacy `.doc` header/footer side-table records split recovered `PlcfHdd`
  stories when available and classify exact even-page, odd-page, and first-page
  header/footer variants, and mirror the first recovered default, even-page,
  and first-page variants into global `DocSetup` running header/footer slots.
- Diagnostics include recovered note and text-box counts alongside other feature
  inventory counts.
- Comments, revisions, fields, shapes, and unsupported constructs are detected
  and exposed through diagnostics, even before all of them are deeply modeled.
- WMF/EMF/EMZ/WMZ package parts are counted as unsupported metafile images, with
  structured metadata for path, format, stored byte size, compression flag, and
  raw or gzip-wrapped header dimensions when available; full metafile rendering
  remains future work.
- Unambiguous `.docx` `REF` bookmark fields, including Word-generated hidden
  bookmark targets, multi-paragraph bookmark ranges, and simple inline
  tabs, line breaks, no-break hyphens, and deterministic `REF \* Upper`/
  `REF \* Lower`/`REF \* Caps`/`REF \* FirstCap` text format switches,
  source-order `REF \p` relative-position results, direct bookmark-name field
  computation when the bookmark exists with supported text-format switches and
  neutral `\h`, explicit numbered-paragraph `\n`/`\n \t`/`\r`/`\r \t`/`\w`/`\w \t`
  from single-branch source paragraphs,
  note-reference `\f`, sequence-separator `\d`, and source-order `\p`, plus
  `REF \n \p` relative suffixes, `REF \n \t` numeric-text suppression,
  `REF \r` relative-context numbered labels with `\r \p` relative suffixes and
  `\r \t` numeric-text suppression when the REF field paragraph also has an
  unambiguous numbering context, and
  `REF \w` full-context numbered labels with `\w \p` relative suffixes and
  `\w \t` numeric-text suppression, `REF \f` visible body footnote/endnote
  reference marks, body comment-reference markers, exact matching bookmarked
  comment ranges, and bookmarks wholly inside body comment ranges, with prior generated REF note marks counted in
  source order, `REF \d "separator"`
  sequence/page separator syntax recognized while preserving cached text until
  sequence/page separator semantics are modeled, bookmarked `NOTEREF`/legacy `FTNREF`
  footnote/endnote reference marks with neutral `\h`, note-reference-style `\f`,
  source-order `\p` above/below results, and common field-result format switches
  when the bookmark encloses a body note reference mark, bare default `TOC`,
  standalone bookmark-scoped default `TOC \b`, explicit
  `TOC \o` heading-outline fields including omitted all-level ranges and common `\o`/`\u` combinations with
  value-neutral `\h`/`\z` switches, text-preserving `\w`/`\x` switches
  normalized to plain text, and text-neutral `\n` no-page-number,
  `\p` entry/page separator, `\s` sequence-number page prefix, and `\d`
  sequence/page separator switches, deterministic TOC `\* Upper`/`\* Lower`/
  `\* Caps`/`\* FirstCap` field-result format switches,
  neutral TOC `\* MERGEFORMAT`/`\* MERGEFORMATINET`/`\* CHARFORMAT`, quoted `TOC \t` custom-style entries,
  `TOC \f` entries from matching `TC "Text"` markers with optional `\f` type
  identifiers and `\l` levels, with supported `TC` marker fields themselves
  rendering as hidden output and unsupported `TC` marker syntax preserving
  cached text with `UnsupportedSwitch` diagnostics,
  `TOC \c` full-caption entries and `TOC \a` label/number-omitted
  caption-text entries from paragraphs containing matching cached
  `SEQ Identifier` fields, and standalone `TOC \u` fields over explicit
  paragraph outline levels, plus
  `TOC \b` bookmark-scoped variants when the bookmark range is recoverable,
  including empty computed results for existing scopes with no matching entries,
  normalize simple inline heading tabs, line breaks, and no-break
  hyphens and expose computed results for simple and common complex field forms;
  cached `MERGEFIELD` results, deterministic simple source-order `SEQ` fields
  with `\n`/`\r`/`\c`, hidden `\h`, and common number-format switches,
  malformed `SEQ` syntax reporting `UnsupportedSwitch`, while valid unsupported
  `SEQ` forms preserve cached text with `NoComputedResult` diagnostics and do
  not mutate later sequence counters, metadata-backed
  document-info fields (`AUTHOR`, `TITLE`, `SUBJECT`, `KEYWORDS`, `COMMENTS`,
  `LASTSAVEDBY`, `CATEGORY`, `CONTENTSTATUS`, `VERSION`, core aliases such as
  `CREATOR`, `DESCRIPTION`, `KEYWORD`, and `LASTMODIFIEDBY`, mapped
  `DOCPROPERTY` names, mapped `INFO` package-property
  subfields, mapped `DOCVARIABLE` names, timestamp-shaped custom
  `DOCPROPERTY` values with simple `\@` pictures, and core timestamp-backed
  `CREATEDATE`/`SAVEDATE`/`PRINTDATE` with simple numeric and English
  month/weekday `\@` pictures, plus app-property-backed
  `NUMPAGES`/`NUMWORDS`/`NUMCHARS`/`EDITTIME`/`TEMPLATE` and common
  scalar built-ins such as `Company`/`Manager`/`HyperlinkBase`/`DocSecurity`
  from `docProps/app.xml`, including direct scalar app-property field names
  such as `APPLICATION`, `APPVERSION`, `COMPANY`, `MANAGER`, `HYPERLINKBASE`,
  `DOCSECURITY`, and `LINKSUPTODATE`), plus `FILESIZE` from the opened
  `.docx` package byte length with raw byte output and rounded `\k`
  kilobyte/`\m` megabyte switches,
  direct `USERNAME`/`USERINITIALS`/`USERADDRESS` fields with explicit quoted
  literal overrides,
  plus cached date/user/unmapped
  document-info fields (`DATE`, `TIME`, unmapped `INFO`, unmapped
  `DOCVARIABLE`, unmapped core date fields, and app stat/template fields
  without backing app properties)
  are named supported display fields for diagnostics when their instruction
  syntax is valid; malformed document-info syntax reports `UnsupportedSwitch`;
  `FILENAME` keeps cached display text, with malformed switches reporting
  `UnsupportedSwitch`; `MERGEFIELD` remains available for template filling,
  with malformed merge-field names reporting `UnsupportedSwitch`; known dynamic/control fields (`=`, `IF`,
  `QUOTE`, `FILLIN`, `ASK`, `COMPARE`, `SET`, `NEXT`, `NEXTIF`, `SKIPIF`) are named
  diagnostics, with deterministic literal arithmetic formula fields computed for
  finite decimal/scientific numeric constants, literal scalar numeric/logical functions (`ABS`, `AND`,
  `AVERAGE`, `COUNT`, `DEFINED`, `FALSE`, `IF`, `INT`, `MAX`, `MIN`, `MOD`, `NOT`, `OR`,
  `PRODUCT`, `ROUND`, `SIGN`, `SUM`, `TRUE`) with comma or semicolon argument
  separators, literal `DEFINED(expr)` checks for parser-local literal expressions,
  `+`, `-`, `*`, `/`, `^`, parentheses, unary signs, literal comparison
  operators (`=`, `<>`, `<`, `<=`, `>`, `>=`), simple non-spanning
  table formulas over existing plain numeric direct A1/RnCn cells plus aggregate
  formulas over positional `LEFT`/`RIGHT`/`ABOVE`/`BELOW`, current `R`/`C`, A1
  cell/range, and RnCn cell/range references, skipping absent cells in ragged rows and including
  nested aggregate calls inside literal formula expressions,
  and simple separated or compact `\#` numeric pictures using `0`/`#`/`x` placeholders, decimal places,
  grouping commas, literal prefix/suffix characters such as `$` or `%`,
  single-section leading `+`/`-` sign-control items, and `x`
  digit-drop/rounding positions, plus two- and three-section
  positive/negative/zero numeric pictures separated by semicolons,
  with optional neutral `\* MERGEFORMAT`/`\* MERGEFORMATINET`/`\* CHARFORMAT` formula tails and
  literal and table formula general-number tails such as `\* DollarText` cents output and integer-valued `\* Hex`/`\* OrdText`,
  malformed formula switch syntax reporting `UnsupportedSwitch`,
  deterministic literal `QUOTE` fields computed from quoted or unquoted text
  using general text-format switches, malformed literal `QUOTE` syntax reports
  `UnsupportedSwitch`, and deterministic literal `IF` fields computed
  for finite decimal/scientific numeric comparisons and quoted string
  equality/inequality, with malformed literal `IF` syntax reporting
  `UnsupportedSwitch`, plus
  deterministic literal `COMPARE` fields computed as `1`/`0` for finite
  decimal/scientific numeric operands and quoted
  `?`/`*` wildcard equality/inequality, deterministic `FILLIN` fields with
  quoted or single-token prompts and explicit `\d` default responses rendered
  without simulating prompts, deterministic `ASK name prompt \d default` fields
  with quoted or single-token prompt/default literals rendered as hidden output
  while feeding later plain `REF`/direct bookmark references and source-order
  bookmark-backed `NEXTIF`/`SKIPIF` comparisons, malformed
  prompt field syntax reports `UnsupportedSwitch`, and deterministic literal
  `SET name "value"` or single-token `SET name value` fields with
  field-result format switches rendered as hidden output while feeding later
  plain `REF`/direct bookmark references and source-order bookmark-backed
  `NEXTIF`/`SKIPIF` comparisons, malformed `SET` syntax reports
  `UnsupportedSwitch`, plus literal `NEXT` and literal or source-order
  bookmark-backed `NEXTIF`/`SKIPIF` merge-control fields with field-result
  format switches
  rendered as hidden output without running a mail merge; malformed
  merge-control syntax reports `UnsupportedSwitch`, invalid literal `COMPARE`
  syntax reports `UnsupportedSwitch`, while remaining dynamic/control
  fields preserve cached display text with `NoComputedResult` until native
  evaluation is implemented; inserted/external-content fields (`INCLUDETEXT`,
  `INCLUDEPICTURE`, `LINK`, `EMBED`, `DATABASE`, `DDE`, `DDEAUTO`, `IMPORT`,
  `INCLUDE`, `AUTOTEXT`, `AUTOTEXTLIST`) are named diagnostics with cached
  display text and `NoComputedResult` until native evaluation is implemented;
  mail-merge helper
  fields (`ADDRESSBLOCK`, `GREETINGLINE`,
  `MERGEREC`, `MERGESEQ`) are named diagnostics with cached display text and
  `NoComputedResult` until native merge-record evaluation is
  implemented; reference/index fields (`BIBLIOGRAPHY`, `CITATION`, `INDEX`,
  `RD`, `TA`, `TOA`, `XE`) are named diagnostics, deterministic simple literal
  `RD`/`TA`/`XE` marker fields render as hidden output, invalid marker syntax
  reports `UnsupportedSwitch`, and generated
  bibliography/citation/index/table-of-authorities fields preserve cached
  display text with `NoComputedResult` until native generation is implemented;
  numbering/list fields compute deterministic
  source-order plain `AUTONUM` values with common number and text format switches and the
  documented `\s` separator switch, including unquoted or quoted
  one-character separators, standalone plain, neutral, common-number-format, or text-format
  `AUTONUMLGL` and `AUTONUMOUT` values on the same source-order counter, plus level-1
  `LISTNUM NumberDefault`/`LegalDefault` values with common number and text format switches, neutral
  field-format switches, and `\s` starts/resets, invalid numbering/list syntax
  reports `UnsupportedSwitch`, while richer `AUTONUMOUT` outline formatting,
  `BIDIOUTLINE`,
  and richer `LISTNUM` levels/named lists are named diagnostics with cached
  display text and `NoComputedResult` until broader native automatic-numbering
  evaluation is implemented;
  document-structure fields (`REVNUM`, `SECTION`, `SECTIONPAGES`,
  `STYLEREF`) are named diagnostics, with `REVNUM` computed from
  `cp:revision`, `SECTION` computed from structural section breaks,
  `SECTIONPAGES` computed for structurally bounded section page counts from
  explicit hard breaks, enabled `pageBreakBefore`, and section starts when they
  do not require layout inference, with page-number and general field-result
  format switches, and deterministic body paragraph-
  and character-style `STYLEREF` computed by style id/name nearest paragraph/run
  lookup, including source-order `\p` above/below and deterministic numbered
  source paragraphs with `\n`, `\r`, `\w`, and numeric-text `\t` switches;
  malformed `REVNUM`/`STYLEREF` syntax reports `UnsupportedSwitch`;
  page-aware/header-footer lookup,
  layout-derived `SECTIONPAGES`, and remaining document-structure cases preserve
  cached display text with `NoComputedResult`;
  display/layout fields (`ADVANCE`, `EQ`, `SYMBOL`) are named diagnostics, with
  deterministic `ADVANCE` fields with validated point movement switches (`\d`,
  `\u`, `\l`, `\r`, `\x`, `\y`) rendered as hidden output while leaving actual
  layout offsets unapplied, validated `EQ \d` displacement controls preserving
  supported operand text, or hidden empty controls, while leaving visual offsets
  and underlines unapplied,
  deterministic `EQ \f(n,d)` simple fractions computed as plain `n/d` text for
  comma or semicolon separators, quoted/spaced operands, documented escaped
  operand characters, and parenthesized nested simple `EQ \f`/`\r` operands,
  simple `EQ \r(radicand)`/`\r(degree,radicand)` radicals computed as plain
  root text, default/custom `EQ \b(element)` brackets with documented `\lc`,
  `\rc`, or `\bc` options computed as bracketed plain text, and
  `EQ \x(element)` boxed operands, including documented border-side options,
  computed as enclosed operand plain text, `EQ \l(...)` lists computed as
  comma-joined operand plain text, simple `EQ \a(...)` arrays computed as
  tab-separated columns and newline-separated rows, simple `EQ \s` scripts
  computed as `^`/`_` marker plain text while preserving non-empty
  `\ai n(...)`/`\di n(...)` operands and hiding empty layout-only controls,
  simple `EQ \i(...)` integrals/summations/products computed as symbol plus
  `_`/`^` limit marker plain text, simple `EQ \o(...)` overstrikes computed as source-order overlay
  operand plain text, plus deterministic `SYMBOL` character insertion computed
  for decimal/hex/default ANSI, Unicode `\u`, neutral `\h`, separated or compact
  font `\f` switches and quoted or unquoted separated/compact size `\s`
  switches, and common
  Symbol/Wingdings font mappings including Symbol `0xB7` bullet; invalid display/layout syntax reports
  `UnsupportedSwitch`, while valid broader display/layout cases preserve
  cached display text with `NoComputedResult`; action/automation fields
  (`GOTOBUTTON`, `MACROBUTTON`, `PRINT`) are named diagnostics, with
  deterministic `GOTOBUTTON`/`MACROBUTTON` quoted or unquoted display text and
  field text-format switches computed without executing navigation or macros,
  validated `PRINT` direct instructions and separated or compact `\p`
  printer-control groups rendered as hidden output without executing
  printer/PostScript instructions; invalid action/automation syntax reports
  `UnsupportedSwitch`, while valid broader action/automation forms preserve
  cached display text with `NoComputedResult`;
  compatibility/private fields (`PRIVATE`, `ADDIN`,
  `DATA`, `GLOSSARY`, `HTMLACTIVEX`) are named diagnostics with cached display
  text and `NoComputedResult` while leaving opaque payloads uninterpreted;
  barcode fields (`BARCODE`, `DISPLAYBARCODE`, `MERGEBARCODE`) are named
  diagnostics with cached display text and `NoComputedResult` until native
  barcode generation is implemented; legacy
  form fields (`FORMTEXT`, `FORMCHECKBOX`, `FORMDROPDOWN`) are named diagnostics
  with deterministic `w:ffData` checkbox checked/default states, dropdown
  result/default selections, explicit non-empty text-input current values, and
  empty-current text-input default computed results where available, while
  explicitly enforced protected-form behavior keeps cached display text with
  `NoComputedResult`;
  body `PAGE` fields compute current page numbers only from
  trusted leading structural or source-rendered current-page context, including
  accepted/current wrappers, single-branch `mc:AlternateContent` page markers,
  trusted section `w:pgNumType` displayed page-number restarts/styles and
  deterministic display-only explicit `w:start` labels for immediate
  section-start `PAGE` fields after visible intro text, deterministic
  page-number format switches plus common
  field-result format switches, while visible-content manual-break and broader layout-derived
  current-page cases preserve cached text with
  `NoComputedResult` diagnostics; `PAGEREF` fields are named, compute page
  numbers only when leading explicit page breaks before any visible body content, enabled
  paragraph `w:pageBreakBefore`, explicit or default `nextPage`, and explicit
  `evenPage`/`oddPage` section starts in leading or trusted rendered context,
  including deterministic display-only `w:pgNumType w:start` page-number
  restart labels and trusted supported `w:pgNumType w:fmt` styles (`decimal`, `decimalZero`,
  `numberInDash`, `decimalFullWidth`, `decimalHalfWidth`, `decimalFullWidth2`,
  `decimalEnclosedCircle`, `decimalEnclosedFullstop`, `decimalEnclosedParen`,
  `ganada`, `chosung`, `koreanDigital`, `koreanCounting`, `koreanLegal`,
  `koreanDigital2`, lower/upper letter, lower/upper roman, ordinal/cardinal
  text) on those section starts and
  single-section final `body/sectPr` page-number defaults,
  source-persisted `w:lastRenderedPageBreak` markers scanned with the same
  single-branch `mc:AlternateContent` policy as flat text, or explicit hard
  breaks after a trusted leading/rendered page context make the target bookmark
  page structural, apply deterministic `\* Arabic`,
  `\* alphabetic`/`\* ALPHABETIC`, `\* roman`/`\* ROMAN`, `\* Ordinal`,
  `\* CardText`, `\* OrdText`, `\* Hex`, integer-valued `\* DollarText`,
  and page-number-only `\* ArabicDash`
  number-format switches plus common field-result format switches, compute `\p` relative results (`above`, `below`, or
  `on page N`) when trusted leading structural page context, source page
  markers, or deterministic display-only restart target/order context provide
  both target and field page/order, and preserve cached page-reference text for
  remaining layout-dependent cases until layout can map bookmarks to emitted pages; cached field results
  preserve simple inline tabs, line breaks, and no-break hyphens for simple and
  common complex body fields;
  unresolved bookmark scopes, unsupported remaining value-changing REF/NOTEREF cases such as
  comment/annotation insertion beyond bookmarked comment-reference markers,
  exact matching bookmarked comment ranges, or bookmarks wholly inside body
  comment ranges, and broader field semantics, existing `NOTEREF`
  bookmark targets without body note-reference marks,
  stay visible through diagnostics with unsupported field-kind counts and
  machine-readable reason counts for unknown fields, unresolved bookmarks,
  unsupported switches, and supported syntax with no computed value, including
  separate missing `PAGEREF` bookmark targets, explicit and direct bookmark-name
  `REF \d` no-computed-result, missing explicit or direct `REF \f` targets, and
  existing explicit or direct non-note `REF \f` unsupported-switch reasons, plus
  separate missing `NOTEREF` targets, existing non-note `NOTEREF` targets, and
  unsupported `NOTEREF` switch reasons, plus separate missing `TOC \b` scopes.

### Editor

- no-op `.docx` `open -> save` preserves retained parts.
- `replace_body_text`, `set_field_result`, `set_comment_text`,
  `fill_content_control_by_tag`, `fill_content_controls_by_tag`,
  `fill_template_fields`, `accept_all_revisions`, `reject_all_revisions`,
  `add_comment_on_text`, `set_hyperlink_target`, `set_table_cell_text`,
  `replace_header_footer_text`, `replace_text_in_part`,
  `add_footnote_on_text`, `add_endnote_on_text`, `replace_note_text`,
  `set_core_property`, and
  `add_image_png` / `replace_image_png` plus
  `add_image_jpeg` / `replace_image_jpeg` and
  `add_image_gif` / `replace_image_gif` plus
  `add_image_bmp` / `replace_image_bmp`,
  `add_image_tiff` / `replace_image_tiff`, and
  `add_image_webp` / `replace_image_webp` remain transactional.
- generated comments, updated comments, and generated or replaced
  footnote/endnote text runs preserve intentional leading/trailing whitespace in
  saved OOXML with `xml:space="preserve"` and emit tabs/newlines as
  WordprocessingML markers.
- Body, accepted-current referenced header/footer, explicit WML part, and
  body-table-cell text replacements emit caller tabs/newlines as
  WordprocessingML markers while preserving unrelated package parts and
  surrounding XML.
- `set_field_result` cached result replacements emit tabs/newlines as
  WordprocessingML markers for simple and common complex field forms.
- package validation catches relationship/content-type inconsistencies,
  including touched internal relationship targets that no longer resolve to a
  retained package part.
- `core_properties`, `edit_capability`, `edited_parts`, and `DocumentReport`
  expose metadata, read-only reasons, and touched package parts before save.
- save failures return `Result` errors rather than producing partial output.

### Authoring

- generated `.docx` files reopen in rdoc, Word, and LibreOffice.
- report example covers styles, tables, images, page setup with section columns,
  document grids, text direction, title pages, page-number restarts/formats, explicit Word document ids, web-extension task pane package shells, explicit page breaks and next/even/odd section breaks,
  default/first/even headers/footers, page numbers, and generated core metadata.
- lossy authoring vs preservation editing is documented.

### Renderer

- render feature builds and tests separately.
- output is valid PDF with selectable text.
- opened `.docx` renders use recovered floating-shape geometry, relative
  z-order, behind/in-front flags, enabled `wp:simplePos` absolute points, anchor
  `dist*` margins, wrap-element `dist*` margins, wrap policy, best-effort
  visible top-level body block page selection including transparent body
  content-control, custom-XML, smart-tag, single-branch `mc:AlternateContent`,
  and accepted/current revision wrappers,
  surfacing direct, inserted, and moved-to shapes while omitting deleted and
  moved-from old-only shapes,
  with feature-inventory counts following those same single-branch and
  accepted/current policies,
  containing-block anchor text plus zero-width anchor character
  offsets, DrawingML preset geometry names, `wp:effectExtent` visual-effect
  bounds, simple sRGB solid fill/outline colors, and text-bearing shape body
  text for bounded approximate overlay previews while keeping exact Office
  layout out of scope.
- validation script reports text recall, page-count ratio, and visual/hash
  metrics against external renderers when configured.
- README labels the renderer as preview/report-grade.

### Quality

- `cargo fmt --all -- --check`;
- `cargo clippy --all-targets -- -D warnings`;
- `cargo test --all-targets`;
- `cargo test --no-default-features`;
- `cargo test --doc`;
- `cargo test --all-targets --features render`;
- corpus scripts can run without private paths;
- release manifests summarize public corpus coverage, render validation, and
  extraction benchmarks without embedding row-level corpus data, while retaining
  compact machine-readable gate results;
- release manifests reject malformed corpus TSV evidence such as empty
  manifests, duplicate columns or paths, negative numeric counts, and duplicate
  warning tokens before embedding totals;
- strict public manifest generation can require existing render-validation and
  extraction benchmark reports plus the exact public `MANIFEST.tsv` and
  `RENDER_MANIFEST.tsv` corpus manifest pair with matching document paths and
  existing listed documents, and reject failed or missing local report gates;
- release workflows publish the generated manifest and packaged crate artifact
  for tagged releases;
- no private data or domain-specific traces in committed examples or fixtures.

## 6. User-Facing Surface

The public API should keep four paths distinct:

- read/query: `Document::open`, `text`, `main_text`, `header_text`,
  `footnote_text`, `endnote_text`, `annotation_text`, `text_box_text`,
  `core_properties`, `comments`, `notes`, `text_boxes`, `floating_shapes`,
  `header_footers`, `model`, `images`, `to_markdown`, `to_html`;
- authoring: `DocBuilder`, `DocModel`, `write_docx`, `try_write_docx`;
- preservation editing: `replace_body_text`, `set_field_result`,
  `fill_content_control_by_tag`, `fill_content_controls_by_tag`,
  `fill_template_fields` for body plus accepted-current referenced header/footer
  content controls and `MERGEFIELD` cached results,
  `accept_all_revisions`, `reject_all_revisions`, `set_hyperlink_target`,
  `set_comment_text`, `add_comment_on_text`,
  `set_table_cell_text` for accepted-current top-level body-table cells using
  `gridSpan`-aware logical columns and `vMerge`-aware logical rows,
  `replace_header_footer_text`,
  `replace_text_in_part`, `add_footnote_on_text`, `add_endnote_on_text`,
  `replace_note_text`, `edit_capability`, `edited_parts`, `set_core_property`,
  `add_image_png`, `replace_image_png`, `add_image_jpeg`,
  `replace_image_jpeg`, `add_image_gif`, `replace_image_gif`,
  `add_image_bmp`, `replace_image_bmp`, `add_image_tiff`,
  `replace_image_tiff`, `add_image_webp`, `replace_image_webp`,
  future tree/edit operations, `save`;
- rendering: `render_pdf`, `try_render_pdf`, `render_pdf_with_fonts`,
  `try_render_pdf_with_fonts`, `render_pdf_with_report`,
  `try_render_pdf_with_report`, `render_pdf_with_fonts_and_report`,
  `try_render_pdf_with_fonts_and_report`, `Document::to_pdf`,
  `Document::try_to_pdf`, `Document::to_pdf_with_fonts`,
  `Document::try_to_pdf_with_fonts`, `Document::to_pdf_with_report`,
  `Document::try_to_pdf_with_report`, `Document::to_pdf_with_fonts_and_report`,
  `Document::try_to_pdf_with_fonts_and_report`.

Do not blur authoring and preservation editing. Authoring creates a fresh Word
package from a model. Preservation editing mutates an existing package while
keeping unmodeled content alive.

## 7. Success Metrics

### Extraction

- corpus-wide panic count: 0;
- text recall against external extractors, measured by script;
- explicit accounting for files below threshold;
- machine-readable extraction benchmark gates for mean POI recall/F1, mean
  LibreOffice recall, scored-file counts, and extraction errors; the
  `public-release` policy fixes optional local extraction trend thresholds at
  `0.95` POI recall/F1 and `0` extractor errors.

### Preservation

- part-payload stability for no-op saves;
- relationship graph validity after edits;
- external openability by python-docx and LibreOffice where available.

### Rendering

- valid PDF output;
- selectable text;
- native preview drawing for authored chart blocks;
- visible placeholders or approximate preview overlays for unsupported preserved
  objects;
- page-count ratio;
- text recall from rendered PDF;
- visual similarity or golden snapshot stability for selected fixtures.
- machine-readable render-validation gates for per-document recall plus optional
  aggregate recall, page-ratio, aHash, warning, and skipped-file thresholds; the
  `public-release` policy fixes optional local render thresholds at `0.97`
  per-document recall, `0.90` aggregate mean recall, and `0` skipped files.

### Project Maturity

- public fixtures are redistributable;
- private benchmark paths are not committed;
- unsupported features are documented and machine-reportable;
- release validation summaries preserve threshold pass/fail metadata and the
  named release policy without copying row-level corpus data;
- release notes distinguish shipped behavior from roadmap claims.

## 8. Current Maturity Gaps

The current product line is broad enough for a public native engine baseline,
but the remaining work is concentrated in deeper compatibility areas rather
than new top-level APIs.

Before support wording moves from diagnostics/cached-result preservation to
computed behavior, each field or layout slice needs deterministic semantics and
focused reader/report coverage. The active native engine backlog is tracked as
public-readable R2 sub-buckets:

- R2-a field report/evaluator parity for value-changing fields whose
  computation, document-report diagnostics, or render-model diagnostics can
  drift;
- R2-b layout-derived `PAGE` and `PAGEREF` outside trusted structural,
  source-rendered, section-start, source-marker, and hard-break contexts;
- R2-c remaining value-changing `REF`, direct bookmark reference,
  `NOTEREF`/`FTNREF`, and TOC/REF body policies where source order, note marks,
  numbering context, or scope membership are not yet unambiguous;
- R2-d broader data, source, layout, generated, action/automation, barcode,
  compatibility/private, and protected legacy-form field families beyond the
  deterministic subsets already implemented, kept cached and reportable until
  deterministic semantics are proven;
- R2-e legacy `.doc` anchors/header-footer, covering exact body, note, text-box,
  and shape anchors plus richer multi-section header/footer application
  semantics beyond current recovered global default/first/even running stories.

The larger non-field maturity gaps are:

- floating-shape exact page/range anchoring, text-wrap reflow, deeper z-order,
  and non-text Office-Art drawing contents;
- newer extension chart families and actual metafile drawing beyond bounded
  diagnostics/header metadata;
- stricter public-release evidence from local render reports and identified
  `extract-vs-mature` extraction benchmark reports before claiming high
  compatibility.

## 9. Risks

### Scope growth

Word formats are large. The mitigation is not to stay small, but to preserve
clear subsystem boundaries: reader, package/editor, authoring, renderer,
diagnostics, and validation.

### False compatibility claims

The renderer can look impressive before it is layout-compatible. Documentation
must keep using measured language.

### Lossy model edits

The `DocModel` is a semantic view. Any edit path that regenerates from it can
discard unmodeled content. Preservation editing must stay package/tree based.

### Corpus contamination

Private, sensitive, or domain-specific documents must stay out of the repository.
Local benchmark scripts may support private corpora by environment variable or
CLI flag only.

## 10. Open Questions

- Should comments and tracked changes become first-class `DocModel` blocks, side
  tables keyed by anchors, or both?
- Should fields expose a common `Field` model across `.doc` and `.docx`, with
  format-specific raw payloads?
- Should renderer validation store golden PDFs, rendered images, hashes, or JSON
  metrics?
- Should the first WASM target expose only read/render, or include editing from
  the start?
