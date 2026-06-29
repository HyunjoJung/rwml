# Roadmap - rdoc native Word engine

This roadmap assumes rdoc is allowed to grow into a mature native Rust Word
document engine, not just a small public crate.

The milestones are ordered so each stage leaves the project more testable and
more honest about what it supports.

## M0 - Public baseline and repository hygiene

**Status:** mostly done.

Goals:

- remove private/domain-specific examples and fixtures;
- keep public fixtures redistributable;
- ensure author metadata is clean;
- keep a single public history when publishing;
- pass formatting, clippy, unit, doc, integration, and render-feature tests.

Exit criteria:

- no known private corpus paths in committed scripts;
- no sensitive or domain-specific sample documents;
- `cargo test --all-targets --features render` passes;
- README scope language matches implementation.

## M1 - Diagnostics and feature inventory

Goal: make unsupported features visible instead of implicit.

Deliverables:

- `Document::report()`;
- `FeatureInventory`;
- `DocumentWarning`;
- counts for comments, notes, text boxes, revisions, fields, hyperlinks,
  shapes, charts, content controls, OLE objects, unsupported image formats, and
  malformed recovered structures;
- JSON output helper for CLI/examples;
- tests with synthetic `.docx` fixtures for each reported feature.

Initial status:

- `Document::report()` exposes format, stats, edit availability, edited package
  part names, core metadata, feature inventory, and warnings.
- `DocumentReport::to_json()` and `examples/diagnose.rs` provide compact JSON
  diagnostics output, including core metadata, without adding a serde
  dependency.
- `.docx` scanning counts comments, tracked-change markers, fields, relationship
  hyperlinks, content controls, floating-shape markers, charts, OLE objects, and
  unsupported metafile parts. Floating-shape feature counts use the same
  accepted/current revision and `mc:AlternateContent` first-branch policies as
  `Document::floating_shapes()`, so direct, inserted, and moved-to shapes count,
  deleted and moved-from old-only anchors or markers are omitted, Choice/Fallback
  serializations of one shape count once, and unrecovered alternate-content shape
  markers still count as one marker. WMF/EMF/EMZ/WMZ parts also expose structured
  diagnostics metadata: package path, format, stored byte size, compression flag,
  and header dimensions for recoverable raw or gzip-wrapped EMF/placeable WMF
  payloads.

Exit criteria:

- opening a complex file tells callers what rdoc saw;
- parity failures can cite missing features rather than guesswork;
- README can include a machine-backed support table.

## M2 - Public corpus expansion

Goal: make quality claims reproducible without private data.

Deliverables:

- synthetic fixture generator for feature-specific `.docx` files;
- small licensed or generated `.doc` fixture set;
- public fixtures for comments, revisions, fields, headers/footers, notes, text
  boxes, nested tables, images, and malformed containers;
- corpus manifest with license/source/kind/expected warnings;
- test that walks the corpus and checks open, report, round-trip, and edit
  behavior where applicable.

Exit criteria:

- public corpus covers the feature inventory;
- every fixture has an expected report;
- private corpus scripts remain opt-in only.

Initial status:

- `corpus/public/MANIFEST.tsv` checks expected `Document::report()` counts and
  warning classes for synthetic comments, notes, text boxes, revisions, fields,
  hyperlinks, nested tables, unsupported object markers, and the broad
  kitchen-sink fixture.
- `scripts/gen_public_corpus.py` deterministically generates focused public
  `.docx` fixtures for comments, revisions, fields, relationship hyperlinks,
  nested tables, unsupported shapes/charts/OLE objects/metafiles, and
  preservation-editor kitchen-sink coverage.

## M3 - `.docx` semantic read depth

Goal: move comments, revisions, and fields from "preserved" to "understood".

Deliverables:

- parse `word/comments.xml`;
- expose comments and anchors in `AnnotationStore`;
- parse simple and complex fields into a `Field` model;
- distinguish hyperlink, PAGE, TOC, FILENAME, MERGEFIELD, REF, PAGEREF,
  document-info/date/stat fields including app-property-backed `EDITTIME`,
  `NUMPAGES`, `NUMWORDS`, `NUMCHARS`, and `TEMPLATE`, dynamic/control fields
  including deterministic literal arithmetic formula fields, literal `QUOTE`,
  literal `IF` comparisons, literal `COMPARE` results, explicit-default
  `FILLIN`/`ASK` prompt fields, and literal `SET`
  bookmark assignments feeding later plain `REF`/direct bookmark references
  and source-order bookmark-backed `NEXTIF`/`SKIPIF` comparisons,
  inserted-content fields, mail-merge
  helper fields, reference/index fields, numbering/list fields, document-structure
  fields, display/layout fields, action/automation fields,
  compatibility/private fields, barcode fields, legacy form fields, and unknown
  fields;
- tracked-change read policies: accepted, original, annotated;
- diagnostics for unsupported field evaluation and incomplete revision views.

Initial status:

- `Document::comments()` extracts the `.docx` comments side table from
  `word/comments.xml` plus optional `word/commentsExtended.xml` reply links
  with id, optional reply parent id, author, initials, date, visible text, and
  body/note/header/footer anchor text when a
  `commentRangeStart`/`commentRangeEnd` pair is present, including visible
  `w:tab`, `w:br`, `w:cr`, `w:noBreakHyphen`, and `w:softHyphen` markers in
  comment bodies and anchors. Comment bodies and anchor text follow the accepted/current
  revision policy, including direct, inserted, and moved-to text while leaving
  deleted and moved-from old-only text out of the visible view.
- `Document::fields()` extracts simple and common complex `.docx` body, note,
  and modeled header/footer fields
  with typed `FieldKind` (`HYPERLINK`, `PAGE`, `TOC`, `FILENAME`, `MERGEFIELD`,
  `REF`, `PAGEREF`, `NOTEREF`, `TC`, `SEQ`, document-info/date/stat fields,
  dynamic/control fields including deterministic literal arithmetic formula
  fields, literal `QUOTE`, literal `IF` comparisons, literal `COMPARE` results,
  explicit-default `FILLIN`/`ASK` prompt fields, and literal quoted or single-token `SET` bookmark assignments feeding later plain `REF`/direct
  bookmark references,
  malformed `SET` syntax reporting `UnsupportedSwitch`,
  inserted-content fields, mail-merge helper fields, reference/index fields,
  numbering/list fields, document-structure fields, display/layout fields,
  action/automation fields, compatibility/private
  fields, barcode fields, legacy form fields, or `Unknown`), normalized instruction, cached visible result text preserving
  inline tabs, line breaks, and no-break/soft hyphens for simple and common
  complex body fields, and
  `computed_result` for unambiguous `.docx` `REF` bookmark targets, including
  Word-generated hidden bookmark targets, multi-paragraph bookmark ranges, and
  inline tabs, line breaks, no-break/soft hyphens for simple and common complex
  body fields, and deterministic
  `REF \* Upper`/`REF \* Lower`/`REF \* Caps`/`REF \* FirstCap` text format switches,
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
  reference marks when the bookmark encloses a body note reference, counting
  prior generated REF note marks in source order plus common field-result
  number/text format switches, `REF \d "separator"`
  sequence/page separator syntax recognized while preserving cached text until
  sequence/page separator semantics are modeled, bookmarked `NOTEREF`/legacy `FTNREF`
  footnote/endnote reference marks with neutral `\h`, note-reference-style
  `\f`, source-order `\p` above/below results, and common field-result
  number/text format switches when the bookmark encloses a body note reference
  mark, bare default `TOC`,
  standalone bookmark-scoped default `TOC \b`,
  explicit `TOC \o` heading-outline ranges including omitted all-level ranges and common `\o`/`\u`
  combinations with value-neutral `\h`/`\z` switches, text-preserving
  `\w`/`\x` switches normalized to plain text, and text-neutral `\n`
  no-page-number, `\p` entry/page separator, `\s` sequence-number page prefix,
  and `\d` sequence/page separator switches, deterministic TOC `\* Upper`/
  `\* Lower`/`\* Caps`/`\* FirstCap` field-result format switches, neutral TOC
  `\* MERGEFORMAT`/`\* CHARFORMAT`, quoted `TOC \t`
  custom-style entries, `TOC \f` entries from matching `TC "Text"` markers with
  optional `\f` type identifiers and `\l` levels, with supported `TC` marker
  fields themselves rendering as hidden output and unsupported `TC` marker
  syntax preserving cached text with `UnsupportedSwitch` diagnostics, `TOC \c` full-caption entries
  and `TOC \a` label/number-omitted caption-text entries from paragraphs
  containing matching cached `SEQ Identifier` fields, and standalone
  `TOC \u` fields over explicit paragraph
  outline levels with normalized simple inline heading tabs, line
  breaks, and no-break/soft hyphens, including common complex begin/separate/end
  fields, deterministic literal arithmetic formula fields with finite
  decimal/scientific numeric constants, literal scalar numeric/logical functions (`ABS`, `AND`, `AVERAGE`,
  `COUNT`, `DEFINED`, `FALSE`, `IF`, `INT`, `MAX`, `MIN`, `MOD`, `NOT`, `OR`, `PRODUCT`,
  `ROUND`, `SIGN`, `SUM`, `TRUE`) with comma or semicolon argument separators,
  literal `DEFINED(expr)` checks for parser-local literal expressions, `+`, `-`, `*`, `/`, `^`, parentheses, unary signs, literal comparison operators (`=`, `<>`, `<`, `<=`, `>`, `>=`),
  simple non-spanning table aggregate formulas over existing plain numeric
  positional `LEFT`/`RIGHT`/`ABOVE`/`BELOW`, current `R`/`C`, A1 cell/range,
  and RnCn cell/range references, skipping absent cells in ragged rows and
  including nested aggregate calls inside literal formula expressions, and simple separated or compact `\#` numeric pictures using `0`/`#`/`x` placeholders,
  decimal places, grouping
  commas, literal prefix/suffix characters such as `$` or `%`,
  single-section leading `+`/`-` sign-control items, and `x`
  digit-drop/rounding positions, plus two- and three-section
  positive/negative/zero numeric pictures separated by semicolons,
  with optional neutral `\* MERGEFORMAT`/`\* CHARFORMAT` formula tails,
  malformed formula switch syntax reports `UnsupportedSwitch`,
  deterministic literal `QUOTE` fields with quoted/unquoted text and general text-format switches,
  deterministic literal `IF` fields for numeric comparisons and quoted string
  equality/inequality, deterministic literal `COMPARE` fields returning `1`/`0`
  including quoted `?`/`*` wildcard equality/inequality,
  deterministic literal `SET name "value"` or single-token `SET name value` fields with
  field-result format switches rendered as hidden output while
  feeding later plain `REF`/direct bookmark references and source-order
  bookmark-backed `NEXTIF`/`SKIPIF` comparisons,
  malformed `SET` syntax reports `UnsupportedSwitch`,
  literal `NEXT` and literal or source-order bookmark-backed `NEXTIF`/`SKIPIF` merge-control fields with
  field-result format switches rendered as hidden output without running a mail merge,
  deterministic `GOTOBUTTON`/`MACROBUTTON` quoted or unquoted display text with
  field text-format switches, and deterministic `w:ffData` `FORMCHECKBOX`
  checked/default states, `FORMDROPDOWN` result/default selections, and
  explicit non-empty `FORMTEXT` current values or empty-current text-input
  defaults.
  Body `PAGE` fields compute current page numbers from trusted leading
  structural or source-rendered current-page context, including
  accepted/current wrappers, single-branch `mc:AlternateContent` page markers,
  section `w:pgNumType` displayed page-number restarts/styles and
  deterministic display-only explicit `w:start` labels for immediate
  section-start `PAGE` fields after visible intro text, deterministic
  page-number format switches plus common field-result format switches, while
  visible-content manual-break and broader layout-derived current-page cases
  preserve cached text with `NoComputedResult`
  diagnostics. `PAGEREF` is
  classified as a named field, computes page numbers only
  when leading explicit page breaks before any visible body content, enabled
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
  single-branch `mc:AlternateContent` policy as flat text, or
  explicit hard breaks after a trusted leading/rendered page context make the
  target bookmark page structural, applies deterministic `\* Arabic`,
  `\* alphabetic`/`\* ALPHABETIC`, `\* roman`/`\* ROMAN`, `\* Ordinal`,
  `\* CardText`, `\* OrdText`, and page-number-only `\* ArabicDash`
  number-format switches plus common field-result format switches, computes `\p` relative results (`above`, `below`, or
  `on page N`) when trusted leading structural page context or source page
  markers provide both target and field page/order, and preserves cached
  page-reference text for remaining layout-dependent cases.
- `Document::notes()` extracts `.docx` footnote/endnote side-table records from
  `word/footnotes.xml` and `word/endnotes.xml` with Word ids, note kind,
  visible text, and reference-id anchors with normalized containing body block
  text when the body references them, including through accepted-current
  body-level revision wrappers.
- `Document::text_boxes()` extracts `.docx` body/note/header/footer text-box
  side-table records from `w:txbxContent` shapes with stable synthetic ids and
  visible text,
  unambiguous anchored-shape containing body anchor text, the same
  `mc:AlternateContent` first-branch policy as flat text, and the
  accepted/current revision policy from the shared body parser.
- `Document::core_properties()` extracts supported `docProps/core.xml` metadata
  fields (`title`, `subject`, `creator`, `description`, `keywords`,
  `category`, `contentStatus`, `lastModifiedBy`, `created`, `modified`,
  `lastPrinted`, `revision`, and `version`) and mirrors `.docx` title/creator
  into `DocSetup`.
- `DocumentReport.custom_properties` exposes parsed string custom document
  properties from `docProps/custom.xml` for diagnostics JSON.
- Generated documents author model-backed `DocSetup` core metadata for
  `title`, `subject`, `creator`, `description`, `keywords`, `category`,
  `contentStatus`, `lastModifiedBy`, `created`, `modified`, `lastPrinted`,
  `revision`, and `version`.
- `Document::report()` includes field-kind counts, and unsupported-field
  warnings now report only missing `PAGEREF` targets, remaining layout-dependent `PAGEREF`, unknown, unresolved bookmark scope, unresolved or
  unsupported remaining value-changing REF cases such as
  comment/annotation insertion and broader REF semantics, missing explicit or
  direct `REF \f` bookmark targets, existing non-note `REF \f` targets,
  missing `NOTEREF` bookmark targets, existing `NOTEREF` bookmark targets without body
  note-reference marks, unsupported `NOTEREF` switches, missing `TOC \b`
  scopes, existing `TOC \b` scopes with no matching entries, and remaining broader TOC field cases whose
  computed values are not evaluated; cached `MERGEFIELD` results, malformed
  merge-field names reporting `UnsupportedSwitch`,
  deterministic simple source-order `SEQ` fields with `\n`/`\r`/`\c`, hidden
  `\h`, and common number-format switches, malformed `SEQ` syntax reporting
  `UnsupportedSwitch`, while valid unsupported `SEQ` forms preserve cached text
  with `NoComputedResult` diagnostics and do not mutate later sequence counters,
  metadata-backed document-info/date results, and cached date/stat/unmapped
  document-info results are counted as supported display
  fields when their instruction syntax is valid; malformed document-info syntax
  reports `UnsupportedSwitch`. The
  known dynamic/control fields (`=`, `IF`, `QUOTE`, `FILLIN`, `ASK`, `COMPARE`,
  `SET`, `NEXT`, `NEXTIF`, `SKIPIF`) are named separately from unknown fields;
  deterministic literal arithmetic formula fields compute finite decimal/scientific numeric constants,
  literal scalar numeric/logical functions (`ABS`, `AND`, `AVERAGE`, `COUNT`,
  `DEFINED`, `FALSE`, `IF`, `INT`, `MAX`, `MIN`, `MOD`, `NOT`, `OR`, `PRODUCT`, `ROUND`,
  `SIGN`, `SUM`, `TRUE`) with comma or semicolon argument separators,
  literal `DEFINED(expr)` checks for parser-local literal expressions, `+`, `-`,
  `*`, `/`, parentheses, unary signs,
  literal comparison operators (`=`, `<>`, `<`, `<=`, `>`, `>=`), simple
  non-spanning table aggregate formulas over existing plain numeric positional
  `LEFT`/`RIGHT`/`ABOVE`/`BELOW`, current `R`/`C`, A1 cell/range, and RnCn
  cell/range references, skipping absent cells in ragged rows and including
  nested aggregate calls inside literal formula expressions, and simple
  separated or compact `\#` numeric pictures using `0`/`#`/`x` placeholders, decimal places, grouping commas,
  literal prefix/suffix characters such as `$` or `%`, single-section leading
  `+`/`-` sign-control items, and `x` digit-drop/rounding positions, plus
  two- and three-section
  positive/negative/zero numeric pictures separated by semicolons,
  with optional neutral `\* MERGEFORMAT`/`\* CHARFORMAT` formula tails,
  malformed formula switch syntax reports `UnsupportedSwitch`,
  deterministic literal `QUOTE` fields compute quoted/unquoted text with
  general text-format switches, malformed literal `QUOTE` syntax reports
  `UnsupportedSwitch`, deterministic literal `IF` fields compute
  finite decimal/scientific numeric comparisons and
  quoted string equality/inequality, malformed literal `IF` syntax reports
  `UnsupportedSwitch`, deterministic literal `COMPARE` fields compute
  `1`/`0` results for finite decimal/scientific numeric operands and either-side
  quoted `?`/`*` wildcard equality/inequality, deterministic `FILLIN` fields
  with quoted or single-token prompts and explicit `\d` default responses
  rendered without simulating prompts, deterministic
  `ASK name prompt \d default` fields with quoted or single-token
  prompt/default literals and field-result format switches rendered as hidden
  output while feeding later plain `REF`/direct bookmark references and
  source-order bookmark-backed `NEXTIF`/`SKIPIF` comparisons, malformed
  prompt field syntax reports
  `UnsupportedSwitch`, and
  deterministic literal `SET name "value"` or single-token `SET name value` fields with
  field-result format switches render as hidden output while
  feeding later plain `REF`/direct bookmark references and source-order
  bookmark-backed `NEXTIF`/`SKIPIF` comparisons,
  malformed `SET` syntax reports `UnsupportedSwitch`, plus literal `NEXT`
  and literal or source-order bookmark-backed `NEXTIF`/`SKIPIF` merge-control fields with
  field-result format switches render as hidden output without running a mail
  merge; malformed merge-control syntax reports `UnsupportedSwitch`, invalid
  literal `COMPARE` syntax reports `UnsupportedSwitch`, while
  remaining dynamic/control fields report `NoComputedResult` until native
  evaluation is implemented. The
  inserted/external-content fields (`INCLUDETEXT`, `INCLUDEPICTURE`, `LINK`,
  `EMBED`, `DATABASE`, `DDE`, `DDEAUTO`, `IMPORT`, `INCLUDE`, `AUTOTEXT`,
  `AUTOTEXTLIST`) are named separately from unknown fields, malformed quoted or
  field-result format syntax reports `UnsupportedSwitch`, and valid broader forms report
  `NoComputedResult` until native evaluation is implemented. Mail-merge helper
  fields (`ADDRESSBLOCK`, `GREETINGLINE`,
  `MERGEREC`, `MERGESEQ`) are named separately from unknown fields, malformed
  quoted or field-result format syntax reports `UnsupportedSwitch`, and valid broader forms
  report `NoComputedResult` until native merge-record evaluation is implemented.
  The
  reference/index fields (`BIBLIOGRAPHY`, `CITATION`, `INDEX`, `RD`, `TA`,
  `TOA`, `XE`) are named separately from unknown fields, deterministic simple
  literal `RD`/`TA`/`XE` marker fields with field-result format switches render
  as hidden output, invalid marker syntax reports `UnsupportedSwitch`, malformed
  generated-field quoted or field-result format syntax reports `UnsupportedSwitch`, and valid broader
  generated bibliography/citation/index/table-of-authorities fields report
  `NoComputedResult` until native generation is implemented. The
  numbering/list fields compute deterministic source-order plain `AUTONUM`
  values with common number and text format switches and the documented `\s` separator
  switch, including unquoted or quoted one-character separators, standalone
  plain, neutral, common-number-format, or text-format `AUTONUMLGL` and `AUTONUMOUT`
  values on the same source-order counter,
  plus level-1 `LISTNUM NumberDefault`/`LegalDefault` values with common
  number and text format switches, neutral field-format switches, and `\s`
  starts/resets, invalid numbering/list syntax reports `UnsupportedSwitch`,
  while richer `AUTONUMOUT`
  outline formatting, `BIDIOUTLINE`, and richer `LISTNUM` levels/named lists
  are named separately from unknown fields and report `NoComputedResult` until
  broader native automatic-numbering evaluation is implemented. The
  document-structure fields (`REVNUM`, `SECTION`, `SECTIONPAGES`, `STYLEREF`)
  are named separately from unknown fields; `REVNUM` computes from
  `cp:revision`, `SECTION` computes the current structural section number,
  `SECTIONPAGES` computes structurally bounded section page counts from
  explicit hard breaks, enabled `pageBreakBefore`, and section starts when no
  layout inference is needed, with page-number and general field-result format
  switches, and deterministic body
  paragraph- and character-style `STYLEREF` computes nearest styled paragraph/run
  text by style id/name plus source-order `\p` above/below and deterministic
  numbered source paragraphs with `\n`, `\r`, `\w`, and numeric-text `\t`
  switches; malformed `REVNUM`/`STYLEREF` syntax reports `UnsupportedSwitch`, while
  page-aware/header-footer,
  layout-derived `SECTIONPAGES`, and remaining document-structure/style lookup
  fields report `NoComputedResult` until native evaluation is implemented. The
  display/layout fields (`ADVANCE`, `EQ`, `SYMBOL`) are named separately from
  unknown fields; deterministic `ADVANCE` fields with validated point movement
  switches (`\d`, `\u`, `\l`, `\r`, `\x`, `\y`) render as hidden output without
  applying layout offsets while accepting field-result format switches, validated `EQ \d` displacement controls preserve
  supported operand text, or hidden empty controls, without applying visual offsets or underlines, deterministic `EQ \f(n,d)` simple fractions with literal,
  spaced, quoted, comma- or semicolon-separated operands plus documented escaped
  comma/semicolon/parenthesis/backslash characters compute plain `n/d` text, nested
  simple `EQ \f`/`\r` operands are parenthesized in plain text, simple
  `EQ \r(radicand)`/`\r(degree,radicand)` radicals compute plain root text,
  default/custom `EQ \b(element)` brackets with documented `\lc`, `\rc`, or
  `\bc` options compute bracketed plain text, and `EQ \x(element)` boxed
  operands, including documented border-side options,
  compute the enclosed operand plain text,
  simple `EQ \l(...)` lists compute comma-joined operand plain text,
  simple `EQ \a(...)` arrays compute tab-separated columns and
  newline-separated rows for supported row-major operands,
  simple `EQ \s` scripts compute `^`/`_` marker plain text while accepting
  empty `\ai n()`/`\di n()` layout controls,
  simple `EQ \i(...)` integrals/summations/products compute symbol plus `_`/`^`
  limit marker plain text,
  simple `EQ \o(...)` overstrikes compute source-order overlay operand plain text,
  deterministic `SYMBOL` fields compute decimal/hex/default ANSI,
  Unicode `\u`, neutral `\h`, separated or compact font `\f` switches and
  quoted or unquoted separated/compact size `\s` switches, and common Symbol/Wingdings character
  insertions including Symbol `0xB7` bullet, with field-result format switches; invalid display/layout syntax reports
  `UnsupportedSwitch`, while valid broader display/layout cases report
  `NoComputedResult` until native layout/equation/symbol evaluation is implemented. The
  action/automation fields (`GOTOBUTTON`, `MACROBUTTON`, `PRINT`) are named
  separately from unknown fields; deterministic `GOTOBUTTON`/`MACROBUTTON`
  quoted or unquoted display text with field text-format switches computes
  without executing navigation or macros, validated `PRINT` direct instructions
  and separated or compact `\p` printer-control groups with field-result format
  switches render as hidden output without executing printer/PostScript
  instructions; invalid action/automation syntax reports `UnsupportedSwitch`,
  while valid broader action/automation forms report `NoComputedResult` without
  executing side effects. The
  compatibility/private fields (`PRIVATE`, `ADDIN`, `DATA`, `GLOSSARY`,
  `HTMLACTIVEX`) are named separately from unknown fields, malformed quoted or
  field-result format syntax reports `UnsupportedSwitch`, and valid broader forms report
  `NoComputedResult` while leaving opaque payloads uninterpreted. The
  barcode fields (`BARCODE`, `DISPLAYBARCODE`, `MERGEBARCODE`) are named
  separately from unknown fields, malformed syntax reports `UnsupportedSwitch`,
  and valid broader forms report `NoComputedResult` until native barcode
  generation is implemented. The
  legacy form fields (`FORMTEXT`, `FORMCHECKBOX`, `FORMDROPDOWN`) are named
  separately from unknown fields; deterministic `w:ffData` checkbox
  checked/default states, dropdown result/default selections, explicit
  non-empty text-input current values, and empty-current text-input defaults
  compute when available with field-result format switches; malformed quoted or
  field-result format syntax reports `UnsupportedSwitch`, while explicitly
  enforced protected-form behavior reports `NoComputedResult`. The
  `.docx` field side table follows the accepted/current revision and
  `mc:AlternateContent` first-branch policies, including direct, inserted, and
  moved-to current fields while omitting deleted and moved-from old fields and
  redundant Choice/Fallback field serializations. The
  feature inventory JSON also includes
  `unsupported_field_reasons` counts for `UnknownField`, `UnresolvedBookmark`,
  `UnsupportedSwitch`, and `NoComputedResult`.
- `Document::revisions()` extracts `.docx` body/note/header/footer insertion,
  deletion, move-from, and move-to markers with id, author, date, and visible
  subtree text.
- `Document::main_text_with_revision_view()` exposes accepted, original, and
  annotated flat-text policies for `.docx` tracked changes.
- The default `.docx` `DocModel`/`main_text()` accepted view descends inline and
  block-level `w:ins`/`w:moveTo` current-content wrappers while continuing to
  omit `w:del`/`w:moveFrom` old-content wrappers.
- `Document::report()` counts tracked property-change revision markers and emits
  `IncompleteRevisionView` because flat text revision views do not reconstruct
  original formatting.

Exit criteria:

- comments are extractable and anchored;
- field instruction/result are visible;
- heavy tracked-change files are explainable;
- no-op save still preserves the original comments/revisions parts.

## M4 - `.doc` semantic parity

Goal: make legacy `.doc` model output less flattened and more region-aware.

Deliverables:

- explicit region model for main body, headers, footers, footnotes, endnotes,
  and text boxes where recoverable;
- field detection in binary `.doc`;
- shape/image placeholders where full rendering is unavailable;
- per-region text APIs backed by the same model instead of ad hoc text slices;
- differential text benchmark improvements against external extractors.

Initial status:

- The legacy `.doc` assembler preserves recoverable field instructions on visible
  field result runs (`FieldRole::Simple`) instead of treating every non-hyperlink
  field as plain text. `Document::fields()` and `Document::report()` now derive
  a normalized field side table and field-kind counts from that model metadata.
- Existing `.doc` hyperlink field behavior remains stable: hyperlink result runs
  still carry `FieldRole::Hyperlink`, and post-field text does not inherit the
  field role.
- `Document::report()` emits `LegacyDocFlattenedSubdocuments` for legacy `.doc`
  inputs whose FIB reports footnote, header/footer, annotation, endnote, or
  text-box character ranges that are still kept in the flat block stream. The
  first recovered header story is mirrored into `DocSetup.header` when
  recoverable, but exact note/text-box body or shape anchors are not yet fully
  promoted.
- `Document::comments()` exposes non-empty legacy `.doc` annotation regions as
  recovered comments with stable synthetic ids, visible comment text, and
  best-effort source-region anchors. Legacy author metadata is not yet
  recovered.
- `Document::notes()` exposes non-empty legacy `.doc` footnote and endnote
  regions as recovered note records with stable synthetic ids, note kind, and
  visible note text plus best-effort source-region anchors. Exact legacy note
  reference anchors are not yet recovered.
- `Document::text_boxes()` exposes non-empty legacy `.doc` text-box regions as
  recovered text-box records with stable synthetic ids, visible text, and
  best-effort source-region anchors. Exact legacy shape anchors are not yet
  recovered.
- `Document::header_footers()` exposes `.docx` running header/footer references
  as exact part/type records with stable `part#type` ids, default
  `Header`/`Footer` kinds, and exact even-page and first-page variants where
  present; paragraph `Block::SectionBreak` setup and final `DocModel::setup`
  store default, first-page, and even-page `.docx` section references, inherit the
  previous default when omitted, and the from-scratch writer emits authored
  first/even variant refs. The renderer selects section-aware
  first/even/default running variants, with first-page variants scoped to each
  section and even variants based on emitted page parity. It also exposes non-empty legacy `.doc` header/footer regions as
  recovered records with stable synthetic ids. When legacy `PlcfHdd` story
  boundaries are present, it splits stories and classifies exact
  `EvenPageHeader`, `OddPageHeader`, `EvenPageFooter`, `OddPageFooter`,
  `FirstPageHeader`, and `FirstPageFooter` records; otherwise it falls back to
  `Unknown` kind.
- Legacy `.doc` region text APIs are now backed by `DocModel::regions`, whose
  source CP spans preserve the exact FIB subdocument boundaries:
  `main_text()`, `header_text()`, `annotation_text()`, `endnote_text()`, and
  `text_box_text()` are separate, while `footnote_text()` preserves its
  historical footnote+endnote convenience without accidentally spanning
  intervening header/annotation ranges.
- `DocModel::regions` records exact legacy `.doc` source-region spans
  (`Main`, `Footnote`, `HeaderFooter`, `Annotation`, `Endnote`, `TextBox`) with
  block ranges, source CP ranges, visible-text spans, and `PlcfHdd` story indexes
  where available; the assembler now emits semantic blocks per FIB subdocument
  region or header/footer story.
- `DocModel::setup.header` mirrors the first non-empty legacy header story, or
  the first non-empty combined `HeaderFooter` source region when no `PlcfHdd`
  story split is available, as a best-effort semantic running-header surface.
- `DocModel::source_regions()`, `source_region_blocks()`,
  `source_region_text()`, and `source_region_kind_text()` provide safe access to
  those spans without requiring callers to hand-slice `model.blocks`.

Exit criteria:

- `model()` can distinguish body vs non-body regions for `.doc`;
- `text()`, `main_text()`, `header_text()`, and note text remain consistent;
- diagnostics explain flattened or unsupported binary structures.

## M5 - Preservation editor expansion

Goal: turn the current safe edit core into a broader editing engine.

Deliverables:

- package transaction layer for multi-part edits;
- relationship/content-type validation before commit;
- text replacement beyond body-only where safe;
- header/footer text replacement;
- add/update comment API;
- set field cached result API;
- table cell text replacement;
- richer image insertion and replacement;
- explicit read-only reasons for packages rdoc refuses to edit.

Initial status:

- `Document::replace_body_text()` rewrites exact body `w:t` matches in the
  retained `word/document.xml` tree, preserves unmodeled siblings, and emits
  replacement tabs/newlines as WordprocessingML markers.
- `Document::set_field_result()` rewrites the cached visible result for simple
  and common complex `.docx` body fields, using the same transactional
  package-preserving edit path as `replace_body_text` and `add_image_png`, and
  emits replacement tabs/newlines as WordprocessingML markers.
- `Document::fill_content_control_by_tag()` and
  `Document::fill_content_controls_by_tag()` fill tagged body `w:sdt` template
  fields by replacing each matching control's visible `w:t` text while preserving
  the control metadata, aliases, tags, and surrounding package; the plural helper
  validates and commits a multi-field template record as one transaction.
- `Document::fill_template_fields()` is the higher-level template/mail-merge
  helper: it fills matching body, note, and accepted-current referenced header/footer
  content-control tags and cached `MERGEFIELD` results by logical field name in
  one package-preserving edit while preserving control metadata and field
  instructions.
- `Document::accept_all_revisions()` accepts tracked body/note/header/footer
  revisions in `word/document.xml`, `word/footnotes.xml`, `word/endnotes.xml`,
  and accepted-current referenced header/footer parts by unwrapping
  inserted/move-to content, removing deleted/move-from content, and dropping
  property-change history while preserving current properties.
- `Document::reject_all_revisions()` rejects tracked body/note/header/footer
  revisions in `word/document.xml`, `word/footnotes.xml`, `word/endnotes.xml`,
  and accepted-current referenced header/footer parts by removing
  inserted/move-to content, unwrapping deleted/move-from content, normalizing
  kept `w:delText` to `w:t`, and dropping property-change history while
  preserving current properties.
- `Document::replace_header_footer_text()` rewrites exact `w:t` matches in
  accepted-current referenced `.docx` header/footer parts while leaving the body
  and unreferenced or old-only header/footer parts untouched, and emits
  replacement tabs/newlines as WordprocessingML markers.
- `Document::replace_text_in_part()` rewrites exact `w:t` matches in one
  explicit existing WordprocessingML XML part; specialized edit APIs remain the
  preferred surface when they can express the semantic operation, and
  replacement tabs/newlines are emitted as WordprocessingML markers.
- `Document::set_comment_text()` rewrites the visible text for an existing
  `.docx` comment while preserving comment metadata and body anchors, and writes
  tabs/newlines as WordprocessingML markers.
- `Document::add_comment_on_text()` creates a `.docx` comment anchored to the
  first exact body text match within one run or adjacent run sequence, creating
  `word/comments.xml`, the comments relationship, and the content type when they
  are absent; newly emitted comment text preserves leading/trailing whitespace
  with `xml:space="preserve"` and writes authored tabs/newlines as
  WordprocessingML markers.
- `Document::set_hyperlink_target()` retargets an existing relationship-backed
  body hyperlink by body order, regenerating only `word/_rels/document.xml.rels`
  while leaving `word/document.xml` byte-preserved.
- `Document::set_table_cell_text()` rewrites visible text in an existing cell of
  an accepted-current top-level `.docx` body table using `w:gridSpan`-aware
  logical columns and `w:vMerge`-aware logical rows; parent cells containing
  nested tables are rejected before mutation, and replacement tabs/newlines are
  emitted as WordprocessingML markers.
- `Document::add_footnote_on_text()` creates `word/footnotes.xml` when needed,
  inserts a body `w:footnoteReference` after the first exact anchor match or
  adjacent run sequence, appends a real `w:footnote`, and validates the footnotes
  relationship/content type before committing the package edit.
- `Document::add_endnote_on_text()` mirrors footnote creation for
  `word/endnotes.xml`, inserting a body `w:endnoteReference` after one run or an
  adjacent run sequence, appending a real `w:endnote`, and validating the
  endnotes relationship/content type before commit.
- Newly emitted footnote/endnote text preserves leading/trailing whitespace with
  `xml:space="preserve"` and writes authored tabs/newlines as WordprocessingML
  markers while normalized convenience text views remain normalized.
- `Document::replace_note_text()` rewrites exact visible text matches in existing
  `.docx` footnote/endnote parts while skipping separator boilerplate; replacement
  tabs/newlines are emitted as WordprocessingML markers.
- `Document::replace_image_png()` replaces an existing `word/media/*.png` part
  with validated PNG bytes while preserving the drawing markup and relationship
  ids that already reference that part.
- `Document::add_image_jpeg()` / `Document::replace_image_jpeg()`,
  `Document::add_image_gif()` / `Document::replace_image_gif()`,
  `Document::add_image_bmp()` / `Document::replace_image_bmp()`,
  `Document::add_image_tiff()` / `Document::replace_image_tiff()`, and
  `Document::add_image_webp()` / `Document::replace_image_webp()` extend the
  same package-preserving image edit path to validated JPEG, GIF, BMP, TIFF,
  and WebP media parts with matching content types.
- `Document::set_core_property()` updates or creates `docProps/core.xml` for
  typed core metadata such as title, creator, category, content status,
  created/modified/last-printed timestamps, revision, and version while
  preserving document body parts;
  `Document::core_properties()` provides the paired read/query surface.
- `Document::edit_capability()` and `DocumentReport.edit` expose typed
  package-preserving edit availability; `PackageReadOnly` diagnostics now report
  legacy `.doc`, incomplete retained packages, and lossy OPC metadata as
  read-only reasons before callers attempt a mutation.
- `Document::edited_parts()` exposes the retained package's sorted touched-part
  set so callers can see which XML, media, relationship, or content-type parts
  an edit has dirtied before saving; `DocumentReport.edited_parts` and report
  JSON expose the same list for diagnostics.
- `Package::to_zip()` validates regenerated/touched `.rels` parts before save:
  internal targets must resolve to retained package parts, while external
  relationships remain allowed.

Exit criteria:

- every edit declares touched parts;
- failures roll back cleanly;
- relationship graph remains valid after edits;
- unmodeled content is preserved or reported.

## M6 - Authoring API maturity

Goal: make generated Word documents ergonomic and credible.

Deliverables:

- `DocBuilder` or builder helpers over `DocModel`;
- style definition API;
- numbering/list API;
- section/page setup API;
- header/footer builder;
- image and hyperlink builder refinements;
- comments, fields, tracked-revision, content-control, and basic chart authoring;
- report examples that cover realistic layouts.

Initial status:

- `DocBuilder` provides thin model-first authoring helpers for core metadata, page
  setup, headings, plain paragraphs, simple text tables with optional header rows,
  layout-aware paragraphs through `ParagraphBuilder`, styled runs through
  `RunBuilder`, named paragraph style definitions through
  `ParagraphStyleBuilder`, width-aware, aligned, fixed-layout, indented, uniform/per-side border-width, border-style, and border-color
  rich tables through `TableBuilder` and `CellBuilder`,
  typed cell paragraphs, explicit cell margins, and nested cell tables through
  `CellBuilder`, list
  paragraphs with explicit levels, simple field runs with cached results,
  run-anchored comments through `CommentBuilder` with reply parent ids,
  commentsExtended metadata, and authored tab/newline markers, inline hyperlink runs through `RunBuilder::hyperlink`,
  tracked insertion/deletion runs through
  `RevisionBuilder`, run-level content controls with data-binding metadata
  through `ContentControlBuilder`, bookmarked runs, authored footnotes/endnotes,
  standalone hyperlink paragraphs, string custom document properties, raw custom
  XML data-store items, image blocks with alt text, explicit sizing, inline
  rotation, and page-relative floating offsets through `ImageBuilder`,
  bar/stacked bar/100% stacked bar/3-D bar/stacked 3-D bar/100% stacked 3-D bar/column/stacked column/100% stacked column/3-D column/stacked 3-D column/100% stacked 3-D column/line/markerless line/smooth line/stacked line/100% stacked line/3-D line/area/stacked area/100% stacked area/3-D area/stacked 3-D area/100% stacked area/radar/radar-with-markers/filled radar/scatter/line-only scatter/smooth scatter/smooth markerless scatter/marker-only scatter/bubble/3-D bubble/pie/exploded pie/3-D pie/exploded 3-D pie/doughnut/exploded doughnut/surface/3-D surface/high-low-close stock/stock/pie-of-pie/bar-of-pie
  charts with embedded workbook-backed data, 3-D bar/column-family shape styling, and
  surface-family wireframe styling through `ChartBuilder`, page
  size/orientation/margins/columns/document grids/text direction/title pages, page-number restarts/formats, explicit Word document ids, web-extension task pane package shells, explicit page breaks, next/even/odd section breaks that snapshot
  page/header/footer setup, styled default/first/even running headers/footers, page numbers, and
  direct `Block` escape hatches.
- Lower-level `DocModel` remains the path for complex nested layouts until the
  builder gains more typed sub-builders.
- Newer extension chart families beyond the current core OOXML authored set and exact chart styling beyond
  3-D bar/column-family shapes and surface-family wireframes remain future
  authoring/rendering work.

Exit criteria:

- users can build a multi-section report without manually filling every struct;
- generated files reopen in rdoc, Word, and LibreOffice;
- authoring docs clearly state that generation is not preservation editing.

## M7 - Renderer fidelity track

Goal: keep improving the native renderer with metrics instead of broad claims.

Deliverables:

- `RenderReport`;
- render warnings tied to `FeatureInventory`;
- page field rendering support where layout context is available;
- font registration and bundled-font option;
- Symbol/Wingdings mapping;
- floating shape placeholders, then basic placement;
- selected golden render fixtures;
- PDF text recall and page-count validation reports.

Initial status:

- `render_pdf_with_report()`, `render_pdf_with_fonts_and_report()`, and
  `Document::to_pdf_with_report()` return generated PDF bytes plus
  `RenderReport` page counts, renderer warnings derived from `FeatureInventory`,
  and model image-byte availability/decode warnings plus placeholder lines when
  images are skipped.
- Opened documents can now use the same explicit font registration path as raw
  `DocModel`s through `Document::to_pdf_with_fonts*` and
  `Document::try_to_pdf_with_fonts*`, including report-returning variants.
- Authored `DocModel` render reports count simple field runs and treat authored
  `Block::Chart` values as supported vector preview charts; opened-document
  reports reuse the full document feature inventory so preserved-but-unmodeled
  shapes/charts/OLE/metafile markers can become renderer warnings.
- Opened-document PDF render paths draw bounded approximate overlay boxes for
  recovered `.docx` floating-shape geometry on the recovered top-level body block
  page when available, and append compact
  placeholder lines for preserved charts, OLE objects, unsupported metafile
  images, image nodes whose bytes are unavailable, skipped raster images whose
  bytes the PDF backend cannot decode, and shape markers without recovered
  geometry. Metafile placeholders are backed by
  the diagnostics inventory metadata, including bounded gzip header inflation
  for EMZ/WMZ dimensions, but payload rendering is not implemented. These are
  honest preview markers, not exact Office layout.
- `Document::floating_shapes()` exposes `.docx` body/note/header/footer `wp:anchor`
  geometry and anchor layout records with `wp:docPr` metadata, `wp:extent`,
  simple
  horizontal/vertical positioning metadata, enabled `wp:simplePos` absolute
  points, `wp:effectExtent` visual-effect bounds, `relativeHeight`,
  behind/in-front flags, anchor `dist*` margins, wrap-element `dist*` margins,
  wrap policy, best-effort visible top-level body block page, containing-block
  anchor text, zero-width anchor character offsets inside that normalized text,
  DrawingML preset geometry names from `a:prstGeom/@prst`, simple sRGB solid
  fill/outline colors, and text-bearing shape body text, using the same
  `mc:AlternateContent` first-branch policy as flat text so DrawingML Choice and
  fallback serializations of the same shape are not duplicated. Shape records and
  anchor block selection now follow transparent body-level content-control,
  custom-XML, smart-tag, and accepted/current revision wrappers: direct,
  inserted, and moved-to shapes are surfaced while deleted and moved-from
  old-only shapes are omitted, so wrapped visible blocks line up with the parsed
  model. Renderer placement now uses those records for approximate
  overlays, recovered simple absolute placement, recovered effect-extents and
  wrap-distance labels, recovered z-order, recovered block-page selection, and
  compact anchor/body-text preview labels; exact body anchor-range page
  resolution, real text-wrap reflow, and full non-text Office-Art drawing
  contents are still future work.
- Generated running footer page numbers (`DocSetup::page_numbers`) and body
  `PAGE` field runs are computed from the actual emitted PDF page list.
  Field-code `HYPERLINK` runs render as link annotations and malformed
  hyperlink syntax reports `UnsupportedSwitch`; body `FILENAME` with malformed
  switches reporting `UnsupportedSwitch`, `MERGEFIELD` with malformed merge-field
  names reporting `UnsupportedSwitch`,
  metadata-backed document-info fields such as
  `AUTHOR`, `TITLE`, `SUBJECT`, `KEYWORDS`, `COMMENTS`, `LASTSAVEDBY`,
  `CATEGORY`, `CONTENTSTATUS`, `VERSION`, core aliases such as `CREATOR`,
  `DESCRIPTION`, `KEYWORD`, and `LASTMODIFIEDBY`, and mapped `DOCPROPERTY` names compute
  from `docProps/core.xml`, `docProps/custom.xml`, or `docProps/app.xml`, mapped `INFO`
  package-property subfields compute from package properties, mapped
  `DOCVARIABLE` names compute from `word/settings.xml`, timestamp-shaped custom
  `DOCPROPERTY` values compute with simple `\@` pictures, and core timestamp-backed
  `CREATEDATE`/`SAVEDATE`/`PRINTDATE` fields compute from core properties with
  simple numeric and English month/weekday `\@` pictures; app-property-backed
  `NUMPAGES`, `NUMWORDS`, `NUMCHARS`, `EDITTIME`, `TEMPLATE`, and common
  scalar built-ins such as `Company`/`Manager`/`HyperlinkBase`/`DocSecurity`
  compute from `docProps/app.xml`, including direct scalar app-property field
  names such as `APPLICATION`, `APPVERSION`, `COMPANY`, `MANAGER`,
  `HYPERLINKBASE`, `DOCSECURITY`, and `LINKSUPTODATE`; `FILESIZE`
  computes from the opened `.docx` package byte length with raw byte output and
  rounded `\k` kilobyte/`\m` megabyte switches; direct
  `USERNAME`/`USERINITIALS`/`USERADDRESS` fields with explicit quoted literal
  overrides compute while environment-backed no-override forms remain cached; and
  remaining cached document-info/date/stat fields
  without backing package properties render without
  unsupported-field warnings when their instruction syntax is valid; malformed
  document-info syntax reports `UnsupportedSwitch`. Known dynamic/control fields are named separately
  from unknown fields; deterministic literal arithmetic formula fields render
  finite decimal/scientific numeric constants and literal scalar numeric/logical functions (`ABS`,
  `AND`, `AVERAGE`, `COUNT`, `DEFINED`, `FALSE`, `IF`, `INT`, `MAX`, `MIN`, `MOD`, `NOT`,
  `OR`, `PRODUCT`, `ROUND`, `SIGN`, `SUM`, `TRUE`) with comma or semicolon
  argument separators, literal `DEFINED(expr)` checks for parser-local literal expressions,
  `+`, `-`, `*`, `/`, `^`, parentheses, unary signs, literal comparison operators (`=`, `<>`, `<`,
  `<=`, `>`, `>=`), simple non-spanning table aggregate formulas over
  existing plain numeric positional `LEFT`/`RIGHT`/`ABOVE`/`BELOW`, current
  `R`/`C`, A1 cell/range, and RnCn cell/range references, skipping absent cells
  in ragged rows and including nested aggregate calls inside literal formula expressions, and simple separated or compact `\#` numeric pictures using
  `0`/`#`/`x` placeholders, decimal places, grouping commas, literal
  prefix/suffix characters such as `$` or `%`,
  single-section leading `+`/`-` sign-control items, and `x`
  digit-drop/rounding positions, plus two- and three-section
  positive/negative/zero numeric pictures separated by semicolons,
  with optional neutral `\* MERGEFORMAT`/`\* CHARFORMAT` formula tails,
  malformed formula switch syntax reports `UnsupportedSwitch`,
  deterministic literal `QUOTE` fields render quoted or unquoted computed text
  with general text-format switches, malformed literal `QUOTE` syntax reports
  `UnsupportedSwitch`, deterministic literal `IF` fields compute finite
  decimal/scientific numeric comparisons and quoted string equality/inequality,
  malformed literal `IF` syntax reports `UnsupportedSwitch`,
  and deterministic literal
  `COMPARE` fields compute `1`/`0` results for finite decimal/scientific numeric operands and either-side quoted `?`/`*` wildcard equality/inequality,
  deterministic `FILLIN` fields with quoted or single-token prompts and explicit `\d` default responses render without simulating prompts,
  deterministic `ASK name prompt \d default` fields with quoted or single-token prompt/default literals and field-result format
  switches render as hidden output while feeding later plain `REF`/direct bookmark references
  and source-order bookmark-backed `NEXTIF`/`SKIPIF` comparisons,
  malformed prompt field syntax reports `UnsupportedSwitch`,
  and deterministic literal `SET name "value"` or single-token `SET name value` fields with
  field-result format switches render as hidden output while
  feeding later plain `REF`/direct bookmark references and source-order
  bookmark-backed `NEXTIF`/`SKIPIF` comparisons,
  malformed `SET` syntax reports `UnsupportedSwitch`,
  plus literal `NEXT` and literal or source-order bookmark-backed `NEXTIF`/`SKIPIF` merge-control fields with
  field-result format switches render as hidden output without running a mail merge;
  malformed merge-control syntax reports `UnsupportedSwitch`; invalid literal
  `COMPARE` syntax reports `UnsupportedSwitch`; remaining
  dynamic/control fields preserve cached text with `NoComputedResult`
  diagnostics.
  Inserted/external-content fields are likewise named separately
  from unknown fields, preserve cached text, report malformed quoted or
  field-result format syntax as `UnsupportedSwitch`, and keep valid broader forms as `NoComputedResult`
  diagnostics. Mail-merge helper fields are named separately from unknown
  fields, preserve cached text, report malformed quoted or field-result format syntax as
  `UnsupportedSwitch`, and keep valid broader forms as `NoComputedResult`
  diagnostics.
  Reference/index fields are named separately from unknown fields; simple
  literal `RD`/`TA`/`XE` marker fields with field-result format switches render
  as hidden output, invalid marker syntax reports `UnsupportedSwitch`, while
  malformed generated-field quoted or field-result format syntax reports
  `UnsupportedSwitch`, and valid
  broader generated bibliography/citation/index/table-of-authorities fields
  preserve cached text with `NoComputedResult` diagnostics.
  Numbering/list fields compute deterministic source-order plain `AUTONUM`
  values with common number and text format switches and the documented `\s` separator
  switch, including unquoted or quoted one-character separators, standalone
  plain, neutral, common-number-format, or text-format `AUTONUMLGL` and `AUTONUMOUT`
  values on the same source-order counter,
  and level-1 `LISTNUM NumberDefault`/`LegalDefault` values with common number
  and text format switches, neutral field-format switches, and `\s` starts/resets; invalid
  numbering/list syntax reports `UnsupportedSwitch`; `BIDIOUTLINE` fields with
  valid field-result format switches and remaining automatic-numbering/list
  fields are named separately from unknown fields and preserve cached text with
  `NoComputedResult` diagnostics.
  Document-structure fields are named separately from unknown fields; `REVNUM`
  computes from `cp:revision`, `SECTION` computes the current structural section
  number, `SECTIONPAGES` computes structurally bounded section page counts when
  no layout inference is needed, including explicit hard breaks, enabled
  `pageBreakBefore`, and section starts, with page-number and general
  field-result format switches, deterministic body paragraph- and
  character-style `STYLEREF` computes nearest
  styled paragraph/run text by style id/name plus source-order `\p`
  above/below and numbered paragraph `\n`, `\r`, `\w`, and numeric-text `\t`
  switch results where numbering context is
  deterministic; malformed `REVNUM`/`STYLEREF` syntax reports `UnsupportedSwitch`, and
  remaining document-structure cases preserve cached text with
  `NoComputedResult` diagnostics.
  Display/layout fields are named separately from unknown fields; deterministic
  hidden validated `ADVANCE`, literal simple `EQ` fractions with supported
  operand separators/escapes and parenthesized nested simple operands, simple
  `EQ \r` radicals, default/custom `EQ \b` brackets, boxed `EQ \x` operands,
  `EQ \l` lists, `EQ \a` arrays, `EQ \s` scripts including empty
  `\ai n()`/`\di n()` layout controls, `EQ \i` integrals/sums/products,
  `EQ \o` overstrikes, operand-preserving or hidden empty `EQ \d` displacement controls, and
  deterministic `SYMBOL` fields compute decimal/hex/default ANSI codepoints,
  Unicode `\u`, neutral `\h`, separated or compact font `\f` switches,
  quoted or unquoted separated/compact size `\s` switches, common
  Symbol/Wingdings mappings including Symbol `0xB7` bullet, and field-result
  format switches; invalid display/layout syntax reports `UnsupportedSwitch`,
  while valid broader display/layout cases preserve cached text with
  `NoComputedResult` diagnostics.
  Action/automation fields are named separately from unknown fields;
  deterministic `GOTOBUTTON`/`MACROBUTTON` quoted or unquoted display text with
  field text-format switches computes without executing actions, validated
  `PRINT` direct instructions and separated or compact `\p` printer-control
  groups with field-result format switches render as hidden output without
  executing printer/PostScript instructions; invalid action/automation syntax
  reports `UnsupportedSwitch`, while valid broader action/automation fields
  preserve cached text with `NoComputedResult` diagnostics.
  Compatibility/private fields are named separately from unknown fields,
  preserve cached text, report malformed quoted or field-result format syntax
  as `UnsupportedSwitch`, and keep valid broader forms as `NoComputedResult`
  diagnostics.
  Barcode fields are named separately from unknown fields, preserve cached text,
  report malformed syntax as `UnsupportedSwitch`, and keep valid broader forms
  as `NoComputedResult` diagnostics.
  Legacy form fields are named separately from unknown fields; deterministic
  `w:ffData` checkbox checked/default states, dropdown result/default
  selections, explicit non-empty text-input current values, and empty-current
  text-input defaults compute when available with field-result format switches;
  malformed quoted or field-result format syntax reports `UnsupportedSwitch`,
  while explicitly enforced protected-form behavior preserves cached text with
  `NoComputedResult` diagnostics.
  Unambiguous
  `.docx` `REF` bookmark fields, including multi-paragraph bookmark ranges and
  inline tabs, line breaks, no-break/soft hyphens for simple and common complex
  body fields, and deterministic
  `REF \* Upper`/`REF \* Lower`/`REF \* Caps`/`REF \* FirstCap` text format switches,
  source-order `REF \p` relative-position results, explicit numbered-paragraph
  `REF \n` labels from single-branch source paragraphs including `\n \p`,
  `\n \t`, `REF \r` relative-context labels including `\r \p` and `\r \t`,
  and `REF \w` full-context labels
  including `\w \p` relative suffixes and `\w \t` numeric-text suppression,
  `REF \f` visible body footnote/endnote reference marks for bookmarks around
  body note references with prior generated REF note marks counted in source
  order plus common field-result number/text format switches,
  `REF \d "separator"` sequence/page separator syntax recognized while
  preserving cached text,
  bookmarked `NOTEREF`/legacy `FTNREF` footnote/endnote reference marks with
  neutral `\h`, note-reference-style `\f`, source-order `\p`
  above/below results, and common field-result number/text format switches,
  direct bookmark-name field
  computation when the bookmark exists with supported text-format switches and
  neutral `\h`, explicit-number `\n`/`\n \t`/`\r`/`\r \t`/`\w`/`\w \t`, note-reference `\f`, sequence-separator `\d`, and source-order `\p`,
  bare default `TOC`,
  standalone bookmark-scoped default `TOC \b`,
  explicit `TOC \o` heading-outline fields including omitted all-level ranges and common `\o`/`\u`
  combinations with value-neutral `\h`/`\z` switches, text-preserving
  `\w`/`\x` switches normalized to plain text, and text-neutral `\n`
  no-page-number, `\p` entry/page separator, `\s` sequence-number page prefix,
  and `\d` sequence/page separator switches, deterministic TOC `\* Upper`/
  `\* Lower`/`\* Caps`/`\* FirstCap` field-result format switches, neutral TOC
  `\* MERGEFORMAT`/`\* CHARFORMAT`, quoted `TOC \t`
  custom-style entries, `TOC \f` entries from matching `TC "Text"` markers with
  optional `\f` type identifiers and `\l` levels, with supported `TC` marker
  fields themselves rendering as hidden output and unsupported `TC` marker
  syntax preserving cached text with `UnsupportedSwitch` diagnostics, `TOC \c` full-caption entries
  and `TOC \a` label/number-omitted caption-text entries from paragraphs
  containing matching cached `SEQ Identifier` fields, and standalone
  `TOC \u` fields over explicit paragraph
  outline levels plus `TOC \b` bookmark-scoped variants when the bookmark range
  is recoverable normalize simple inline heading tabs, line breaks,
  and no-break/soft hyphens, expose computed results, and display the computed text
  for simple and common complex fields. `PAGE` computes current page numbers
  from trusted leading structural and source-rendered current-page contexts,
  including accepted/current wrappers, single-branch `mc:AlternateContent`
  page markers, deterministic display-only explicit `w:start` labels for
  immediate section-start `PAGE` fields after visible intro text, page-number
  format switches, and common field-result format
  switches, preserving cached text for broader layout-derived cases with
  `NoComputedResult` diagnostics. `PAGEREF` computes
  target pages only from leading explicit page breaks before visible body content, enabled
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
  source-persisted `w:lastRenderedPageBreak` markers, or
  explicit hard breaks after a trusted leading/rendered page context, applies
  deterministic page-number and field-result format switches, computes trusted leading-structural
  and source-marker `\p` relative-position results, and preserves cached text for remaining
  layout-derived cases. Document and render
  diagnostics still report unknown, unresolved bookmark scope, unresolved or
  unsupported remaining value-changing REF cases such as
  comment/annotation insertion and broader REF semantics, missing explicit or
  direct `REF \f` bookmark targets, existing non-note `REF \f` targets,
  missing `NOTEREF` bookmark targets, existing `NOTEREF` bookmark targets without body
  note-reference marks, unsupported `NOTEREF` switches, remaining layout-dependent `PAGEREF`,
  missing `TOC \b` scopes, existing `TOC \b` scopes with no matching entries,
  and broader TOC body fields as unsupported
  evaluation while preserving cached field-result inline tabs, line breaks, and
  no-break/soft hyphens, and they expose reason counts alongside unsupported field
  kind counts, distinguishing missing `PAGEREF` targets,
  explicit and direct bookmark-name `REF \d` supported syntax with no computed
  result, missing explicit or direct `REF \f` targets, and existing explicit or
  direct non-note `REF \f` unsupported-switch cases, missing `NOTEREF` targets,
  existing non-note `NOTEREF` targets, missing `TOC \b` scopes, existing
  empty `TOC \b` scopes, and
  truly unresolved bookmarks.
- The renderer maps common Symbol/Wingdings legacy font code points to Unicode,
  including the Symbol `0xB7` bullet,
  before PDF shaping. The mapping is intentionally partial and render-only:
  model text, `text()`, and exporters preserve the original source code points.
- `scripts/render_validate.py --json` emits a machine-readable render validation
  report with per-document text recall, page counts, page-count ratio,
  average-hash similarity, render-warning counts/kinds, skips, and aggregate
  means for release tracking. `examples/to_pdf --report-json` writes the
  per-render `RenderReport` sidecar used by that validator.
- Fallible render APIs (`try_render_pdf*`, `Document::try_to_pdf*`) surface PDF
  serialization errors through `rdoc::Error::Render`; the legacy infallible
  wrappers remain for compatibility.
- `corpus/public/RENDER_MANIFEST.tsv` and `tests/public_corpus.rs` now provide a
  render-feature fixture gate for synthetic documents: PDF bytes must be emitted,
  page counts must stay stable, and render warnings must match the manifest.

Exit criteria:

- renderer failures are reported as feature warnings;
- generated PDFs remain valid and selectable;
- page count and text recall trends are tracked;
- README continues to call it preview/report-grade unless metrics justify more.

## M8 - WASM and demo surface

Goal: expose the mature core in browser contexts after diagnostics are stable.

Deliverables:

- WASM build profile;
- read/report API in WASM;
- optional render-to-SVG or render metadata output;
- browser demo for open, inspect, extract, and preview;
- no editing UI until edit transactions and diagnostics are robust.

Exit criteria:

- browser demo can load public fixtures;
- unsupported features are visible in the UI;
- WASM does not become a second core implementation.

Initial status:

- The crate emits both `rlib` and `cdylib` artifacts, and `wasm32` builds pull in
  `wasm-bindgen` only as a target-specific dependency.
- `rdoc::wasm` exposes thin read/report adapters (`extractText`, `markdown`,
  `html`, `reportJson`) that call `Document::open`, the normal exporters, and
  `DocumentReport::to_json()` instead of creating a browser-only parser.
- `examples/wasm-demo` provides a static browser inspector for local `.doc` and
  `.docx` files, surfacing text, Markdown, HTML preview, diagnostics JSON,
  observed feature markers, and warnings. It intentionally has no editing UI.

## M9 - Productization and ecosystem

Goal: make rdoc useful outside the crate API.

Possible deliverables:

- CLI binary;
- JSON diagnostics command;
- conversion command;
- optional Python/Node bindings;
- editor/viewer integrations;
- release artifacts with checksums;
- benchmark reports published per release.

Exit criteria:

- product layers consume the same Rust core;
- releases include validation summaries;
- downstream users can reproduce corpus checks.

Initial status:

- The `rdoc` CLI binary exposes the core library through `extract`, `diagnose`,
  `convert`, `to-docx`, and render-gated `to-pdf` commands. `diagnose` emits the
  same compact JSON as `DocumentReport::to_json()`, `to-docx` converts through
  `Document::open()` -> `DocModel` -> `write_docx()`, and `to-pdf` writes native
  PDF output with an optional `--report-json` sidecar from `RenderReport`.
- CLI integration tests exercise the installed binary path with synthetic `.docx`
  fixtures for text extraction, JSON diagnostics, Markdown/HTML/text conversion,
  reopenable `.docx` output, and render-feature PDF/report output.
- `scripts/release_manifest.py` creates deterministic release manifest JSON with
  artifact byte sizes, SHA-256 checksums, version/git metadata, named
  `public-release` policy metadata, summary sections from validation reports such
  as `scripts/render_validate.py --json`, compact public hygiene,
  validation/benchmark gate metadata, compact public corpus TSV manifest
  summaries with document counts, numeric totals, and warning counts. Corpus TSV
  summaries reject empty manifests, duplicate columns or paths, negative numeric
  counts, and duplicate warning tokens before embedding totals. Its
  `release_evidence` metadata distinguishes embedded policy,
  complete-but-not-enforced inputs, enforced strict local evidence, and missing
  strict public-release inputs, including invalid public corpus manifests and
  manifest pairs whose document path lists do not match. It also provides
  `--enforce-policy-inputs` validation that requires a passing public hygiene
  report, render validation, extraction benchmark, and exactly the public
  `MANIFEST.tsv` plus `RENDER_MANIFEST.tsv` corpus manifest pair with matching
  document paths and rejects failed hygiene/validation/benchmark gates or
  validation/benchmark reports generated with weaker thresholds than the named
  `public-release` policy.
- `scripts/public_hygiene_audit.py` statically scans committed plus untracked,
  non-ignored files for public-release blockers: non-public corpus files,
  known domain-specific project traces, absolute local home paths, private
  corpus env var assignments, and high-risk secret token literals. It also
  scans bounded decoded byte text views from legacy `.doc` files, rejecting
  oversized legacy binaries rather than passing them uninspected. For Office OPC
  packages such as `.docx` and `.xlsx`, it scans textual internal parts such as
  core properties, relationships, content types, WordprocessingML XML, and
  embedded Office package XML such as chart workbooks while leaving binary media
  opaque.
- `scripts/bench_vs_mature.py --json` now emits a schema-tagged extraction
  benchmark report with release metadata, rows, and aggregate recall/F1 summary
  metrics against local Apache POI and LibreOffice goldens. It can enforce
  release thresholds for mean POI recall/F1, mean LibreOffice recall, scored-file
  counts, and extractor errors. Release manifests can attach one or more
  benchmark reports by summary/gate only.
- `scripts/render_validate.py --json` emits a compact `gate` section for the
  existing per-document recall threshold and optional aggregate render thresholds
  for mean recall, page-ratio, aHash similarity, warning count, and skipped files.
- `.github/workflows/release.yml` runs the public hygiene audit plus the required
  fmt/clippy/default test/doc gates and the render-feature test gate on
  tag/manual release runs, packages the crate, generates
  `dist/rdoc-release-manifest.json` from the packaged `.crate` artifact plus
  public hygiene and public corpus manifest summaries plus `public-release`
  policy, and uploads the
  manifest and crate package as workflow artifacts before crates.io publishing.

## Running Principles

- Preserve before interpreting.
- Report before silently dropping.
- Keep `DocModel` semantic, not a fake source package.
- Keep authoring and preservation editing separate.
- Make renderer progress measurable.
- Prefer public fixtures; keep private corpus hooks local and explicit.
- Add APIs only when their failure modes are clear.

## Near-Term Cut

The next useful implementation batches are ordered by evidence. Do not reopen
old writer-gap claims without checking current code and tests first; the writer,
authoring charts, edit layer, diagnostics, and release tooling are already broad
enough that most remaining work is field semantics, layout, fixtures, or
validation depth.

For parser/evaluator/report work, prefer one bounded parity batch at a time:
share duplicated helpers or fix a proven report/evaluator drift, verify with a
focused field/report test pair, and avoid changing cached-result policy unless
the semantics are deterministic.

The active roadmap slices are:

1. Field evaluation backlog, ordered for focused slices:
   - parser/evaluator/report parity for value-changing fields where exact
     duplicate syntax logic remains or diagnostics disagree with computation;
   - layout-derived `PAGE`/`PAGEREF` beyond trusted leading/source-rendered,
     section-start, source-marker, and hard-break contexts: exact pagination
     current-page/page-reference values, target-derived formatting where no
     trusted marker exists, and remaining layout-dependent `\p` results;
   - remaining value-changing reference policy: comment/annotation insertion,
     broader `REF` semantics, unresolved or unsupported `NOTEREF` switches
     beyond body note-reference marks, and broader TOC/REF body evaluation;
   - remaining data-, source-, layout-, action-, and generated-field families
     beyond the deterministic subsets already listed above, including
     unknown fields, dynamic/control fields that need external state or side
     effects, generated reference/index output beyond hidden literal
     `RD`/`TA`/`XE` markers, richer numbering/list semantics, display/layout
     fields beyond the deterministic `ADVANCE`/`EQ`/`SYMBOL` subset,
     action/automation beyond display text and validated hidden `PRINT`,
     compatibility/private payloads, barcode rendering, and protected legacy
     form behavior.
   - For each slice, preserve cached text until semantics are unambiguous,
     distinguish `UnsupportedSwitch` from `NoComputedResult`, and add focused
     `.docx` and report diagnostics tests before public support wording moves.
2. Continue legacy `.doc` exact body/shape anchors beyond the current
   source-region anchors for comments, notes, and text boxes, plus richer legacy
   section-level header/footer application semantics beyond recovered/default
   running stories.
3. Floating-shape placement beyond geometry overlays: exact body anchor
   range/page resolution beyond best-effort top-level block pages, real
   text-wrap reflow, deeper z-order semantics beyond recovered
   `relativeHeight`/`behindDoc`, and non-text Office-Art drawing contents.
4. Newer extension chart families and metafile rendering strategy beyond the current
   bounded diagnostics/header metadata, with public fixtures before broad claims.
5. Release validation policy: keep tightening which external, locally configured
   render/extraction benchmark reports are required for public releases. The
   `public-release` manifest policy now fixes the required Rust gates and optional
   threshold values (`0.95` POI recall/F1, `0.90` render mean recall, `0` errors
   or skips), and `scripts/release_manifest.py --enforce-policy-inputs` can make
   render validation, extraction benchmark, and the exact public
   `MANIFEST.tsv`/`RENDER_MANIFEST.tsv` corpus pair with matching document paths
   mandatory with passing compact gates and policy-strength thresholds for
   strict public manifests.
   Manifests now record `release_evidence.strict_policy_status`,
   `strict_policy_enforced`, `strict_policy_inputs_complete`, and missing
   strict inputs so tagged automation that only embeds the policy remains
   distinguishable from strict local release evidence. Tagged automation
   intentionally emits the non-strict policy manifest until local render and
   extraction reports are generated in the workflow; strict public manifests
   remain an explicit local generation step.

These are deliberately deeper roadmap items: the diagnostics, corpus, `.docx`
comment/field/revision side tables, metadata query surface, preservation edits,
authoring builders, render reports, WASM demo, and CLI layers now exist and
should be extended rather than restarted.
