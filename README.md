# rwml

[![Crates.io](https://img.shields.io/crates/v/rwml.svg)](https://crates.io/crates/rwml)
[![Docs.rs](https://docs.rs/rwml/badge.svg)](https://docs.rs/rwml)
[![CI](https://github.com/HyunjoJung/rwml/actions/workflows/ci.yml/badge.svg)](https://github.com/HyunjoJung/rwml/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![MSRV](https://img.shields.io/badge/MSRV-1.85%20(render%201.92)-orange.svg)

**rwml** takes its name from **WordprocessingML**, the OOXML markup for Word
documents. It is a native Rust toolkit to **read**, **write**, **edit**, and
**render** both legacy **`.doc`** (Word 97–2003 binary, [MS-DOC]) and modern
**`.docx`** (OOXML WordprocessingML). No JVM, no Apache POI, no other `.docx`
crate, and no subprocess.

One model ties it together. [`Document::open`] detects the format from the magic
bytes (OLE2 `D0CF11E0` → `.doc`; ZIP `PK` → `.docx`) and both backends produce the
**same** [`DocModel`]; the Markdown/HTML exporters, the `.docx` writer, and the PDF
renderer all consume that one model, so your code never branches on the format.

```text
 .doc  ┐                          ┌→ text · markdown · html
 .docx ┼→ Document::open → DocModel┼→ write_docx  (styled .docx)
 (build it yourself) ┘             ├→ render_pdf   (typeset PDF)
                                   └→ edit → save  (package-preserving .docx)
```

## Read

```rust
// Plain text (search / indexing) — .doc or .docx, detected automatically:
let bytes = std::fs::read("report.docx")?;
let text = rwml::extract_text(&bytes)?;

// Rich model + exporters (identical IR for .doc and .docx):
let doc   = rwml::Document::open(&bytes)?;
let md    = doc.to_markdown();   // # headings, **bold**, | tables |, lists, links, page breaks
let html  = doc.to_html();       // <h1>, <strong>, <table colspan>, <img>, <a>, page breaks
let model = doc.model();         // typed IR: Vec<Block> (Paragraph | Table | Image | PageBreak | SectionBreak)
let hregs = model.source_regions(rwml::SourceRegionKind::HeaderFooter);
let imgs  = doc.images();        // extracted raster bytes (like POI getAllPictures)
let info  = doc.report();        // format, stats, edit state, feature inventory incl. notes/text boxes/metafiles
let json  = info.to_json();      // compact diagnostics JSON for scripts/CLI
let edit  = doc.edit_capability(); // package-preserving edit availability
let props = doc.core_properties(); // title/creator/etc. from docProps/core.xml when present
let comments = doc.comments();   // .docx comments + recovered .doc annotations
let notes = doc.notes();         // .docx + recovered .doc footnote/endnote records with anchors
let boxes = doc.text_boxes();    // .docx + recovered .doc text-box records
let shapes = doc.floating_shapes(); // .docx wp:anchor geometry/layout/anchor/preset/color/simplePos/effect/wrap-distance/polygon records
let hfs   = doc.header_footers(); // .docx part/type records + recovered .doc regions
let flds  = doc.fields();        // .docx + recovered .doc fields
let revs  = doc.revisions();     // .docx tracked changes (kind, metadata, text)
let hdr   = doc.header_text();   // running header/footer text when modeled
let orig  = doc.main_text_with_revision_view(rwml::RevisionView::Original);
```

## Write — author a styled `.docx`

Build a document with [`DocBuilder`] or the lower-level [`DocModel`] structs, then
serialize it to a clean, Office-openable `.docx`. Character formatting (font,
size, color, bold/italic, highlight, super/subscript), paragraph layout
(named styles, headings, alignment, spacing, indent, shading, page-break-before), leveled lists,
**bordered tables with width, fixed layout, alignment, indentation, uniform/per-side border widths, styles, and colors, and per-cell shading / width / margins / vertical alignment**, images
with alt text, explicit pixel size, inline rotation, and page-relative floating
offsets, simple fields with cached results, `PAGEREF` helper runs, dirty TOC
heading-range fields, run-anchored comments with reply parent ids and
commentsExtended metadata, tracked
insertion/deletion runs, run-level content controls, bookmarked runs, authored
footnotes/endnotes, string custom document properties, raw custom XML data-store
items, generated core metadata (title, subject, creator, description, keywords,
category, content status, last modified by, created, modified, last printed,
revision, and version), explicit Word document ids, web-extension task pane package shells, page setup with section columns, document grids, text direction, title pages, and page-number restarts/formats, explicit page breaks and next/even/odd section breaks,
styled default/first/even running headers/footers, and page numbers all round-trip. Content controls can include tag/alias and
data-binding metadata.

<a name="chart-families"></a>`ChartBuilder` authors the current core OOXML
**chart families** with embedded workbook-backed data: bar / column / line / area
(each in plain, stacked, 100%-stacked, and 3-D variants), radar (plain,
with-markers, filled), scatter (line-only, smooth, smooth-markerless,
marker-only), bubble (2-D and 3-D), pie / doughnut (plain, exploded, 3-D),
surface / 3-D surface, high-low-close stock and stock, and
pie-of-pie / bar-of-pie. It also authors the newer **chart-ex extension
families** — waterfall, treemap, sunburst, histogram, box & whisker, and
funnel — emitted as `chartEx` parts. `wireframe()` styling is available for
surface-family charts and `ChartShape` styling (cylinder/pyramid) for 3-D
bar/column-family charts. See [`examples/report.rs`](examples/report.rs).

```rust
let model = rwml::DocBuilder::new()
    .title("분기 운영 리포트")
    .creator("rwml")
    .margins_pt(54.0)
    .header_runs([rwml::RunBuilder::new("분기 운영 리포트").bold().build()])
    .footer_runs([rwml::RunBuilder::new("Page ").italic().build()])
    .page_numbers()
    .paragraph_style(
        rwml::ParagraphStyleBuilder::new("RiskCallout", "Risk callout")
            .based_on("Normal")
            .shading(rwml::Color::rgb(0xFE, 0xF2, 0xF2))
            .run_bold()
            .run_color(rwml::Color::rgb(0xC0, 0x00, 0x00)),
    )
    .heading(1, "분기 운영 리포트")
    .paragraph("작성일 2026-06-22")
    .rich_paragraph(rwml::ParagraphBuilder::new().runs([
        rwml::RunBuilder::new("주의 필요")
            .comment(
                rwml::CommentBuilder::new("담당자 확인 필요")
                    .author("Reviewer")
                    .initials("RV"),
            )
            .build(),
        rwml::RunBuilder::new(" - ").build(),
        rwml::RunBuilder::new("가이드")
            .hyperlink("https://example.com/guide")
            .underline()
            .build(),
        rwml::RunBuilder::new("추가 문장")
            .revision(
                rwml::RevisionBuilder::insertion()
                    .author("Reviewer")
                    .date("2026-06-24T01:00:00Z"),
            )
            .build(),
        rwml::RunBuilder::new("승인 필요")
            .content_control(
                rwml::ContentControlBuilder::new()
                    .alias("Approval")
                    .tag("approval-required"),
            )
            .build(),
    ]).style("RiskCallout"))
    .numbered_list(["문서 변환 점검", "릴리스 노트 작성"])
    .bullet_list_level(1, ["담당자 확인"])
    .field("FILENAME \\p", "report.docx") // writes a simple field cached result
    .hyperlink("프로젝트 링크", "https://example.com/")
    .rich_table(
        rwml::TableBuilder::new()
            .header_rows(1)
            .col_widths_pct([0.7, 0.3])
            .row([
                rwml::CellBuilder::text("작업")
                    .shading(rwml::Color::rgb(0x1F, 0x38, 0x64)),
                rwml::CellBuilder::text("담당 부서")
                    .shading(rwml::Color::rgb(0x1F, 0x38, 0x64)),
            ])
            .row([
                rwml::CellBuilder::text("문서 변환 점검"),
                rwml::CellBuilder::text("플랫폼팀"),
            ]),
    )
    .section_break()
    .clear_header()
    .page_size_pt(792.0, 612.0)
    .landscape()
    .header_runs([rwml::RunBuilder::new("후속 조치").bold().build()])
    .heading(2, "후속 조치")
    .build();

std::fs::write("out.docx", rwml::write_docx(&model))?;   // opens in Word & LibreOffice
```

The output is validated to re-open in **Word** (verified via python-docx reading
back the named styles, run colors, fonts, and table shading) and **LibreOffice**.

## Edit — open, change, save (package-preserving)

`Document::open` keeps the whole package, so `save()` re-emits it with everything
rwml doesn't model preserved verbatim (themes, settings, fonts, comments, custom
XML, charts, embeddings, unknown parts). A no-op open→save is byte-stable per part.

```rust
let mut doc = rwml::Document::open(&std::fs::read("in.docx")?)?;

// Element-tree edit: preserves fields, content controls, shapes, comments…
doc.replace_body_text("DRAFT", "FINAL")?;
doc.set_field_result(0, "7")?;                  // cached result for body field index 0
let note_fields = doc.fields_in_part("word/footnotes.xml")?;
if !note_fields.is_empty() {
    doc.set_field_result_in_part("word/footnotes.xml", 0, "Updated note field")?;
} // explicit part-local index; also supports correctly typed header/footer parts
let blocks = doc.body_blocks()?;                 // direct p/tbl/sdt edit indices + kinds
if blocks.len() >= 3 {
    doc.move_body_block(blocks[0].index, 2)?;    // exact subtree, final index 2
    doc.remove_body_block(1)?;                   // conservative exact-subtree removal
}
let append_position = doc.body_blocks()?.len(); // re-enumerate after structural edits
doc.insert_body_paragraph(
    append_position,
    "Appended plain paragraph",
)?;                                              // before final body sectPr
doc.fill_content_controls_by_tag([
    ("client-name", "Acme & Co"),
    ("project-name", "Roadmap"),
])?;
doc.fill_template_fields([
    ("client-name", "Acme & Co"),
    ("project-name", "Roadmap"),
])?; // body/note/header/footer content controls + MERGEFIELD cached results
doc.accept_all_revisions()?;                    // accept tracked body/note/header/footer changes
// doc.reject_all_revisions()?;                 // or reject tracked body/note/header/footer changes
doc.set_hyperlink_target(0, "https://example.com/final")?; // body hyperlink rel
doc.set_comment_text("7", "Updated note")?;     // existing comment body text
doc.add_comment_on_text("Clause", "Check this", "Reviewer")?; // exact body run anchor
doc.set_table_cell_text(0, 0, 1, "Updated")?;   // top-level table/row/logical column
doc.replace_header_footer_text("DRAFT", "FINAL")?;
doc.replace_text_in_part("word/header2.xml", "DRAFT", "FINAL")?; // explicit WML part
doc.add_footnote_on_text("Clause", "Source note")?; // exact body run anchor
doc.add_endnote_on_text("Clause", "Appendix note")?; // exact body run anchor
doc.replace_note_text("DRAFT", "FINAL")?;       // existing footnote/endnote text
doc.add_image_png(&png_bytes, "image1.png")?;   // media + content-type + rId, atomic
doc.replace_image_png(&new_png, "image1.png")?; // existing word/media/*.png bytes
doc.add_image_jpeg(&jpg_bytes, "photo.jpg")?;   // validated JPEG media insert
doc.replace_image_jpeg(&new_jpg, "photo.jpg")?; // existing word/media/*.jpg bytes
doc.add_image_gif(&gif_bytes, "anim.gif")?;     // validated GIF media insert
doc.replace_image_gif(&new_gif, "anim.gif")?;   // existing word/media/*.gif bytes
doc.add_image_bmp(&bmp_bytes, "bitmap.bmp")?;   // validated BMP media insert
doc.replace_image_bmp(&new_bmp, "bitmap.bmp")?; // existing word/media/*.bmp bytes
doc.add_image_tiff(&tiff_bytes, "scan.tiff")?;  // validated TIFF media insert
doc.replace_image_tiff(&new_tiff, "scan.tiff")?; // existing word/media/*.tif/.tiff bytes
doc.add_image_webp(&webp_bytes, "pic.webp")?;   // validated WebP media insert
doc.replace_image_webp(&new_webp, "pic.webp")?; // existing word/media/*.webp bytes
doc.set_core_property(rwml::CoreProperty::Title, "Final report")?;

let touched = doc.edited_parts();               // package parts dirtied by edits
std::fs::write("out.docx", doc.save()?)?;        // untouched parts preserved
```

Body content-control fills, plus logical template fills in their currently
supported body, note, and referenced header/footer locations, encode input `\t`
and `\n` as WordprocessingML `w:tab` and text-wrapping `w:br` run content.
Repeated fills clear prior text-wrapping markers, including after save/reopen;
marker-only values written by these APIs retain an empty `w:t` refill anchor.
Newlines remain inline breaks rather than new paragraphs, while existing
page/column breaks, other run objects, data bindings, and rich-run structure
remain preserved rather than interpreted or redistributed.

Several existing edit methods can share one package rollback boundary:

```rust
let mut doc = rwml::Document::open(&std::fs::read("in.docx")?)?;
let mut edits = doc.edit_session()?;
edits.replace_body_text("DRAFT", "FINAL")?;
edits.set_core_property(rwml::CoreProperty::Title, "Final report")?;
edits.commit()?; // validates, refreshes read views, and retains both edits
```

`EditSession` snapshots the complete retained package and its existing
touched-part state. Individual operation failures remain non-poisoning because
each edit method is already atomic; a handled error may be followed by more
operations, but any session not explicitly committed rolls back. Read views stay
at their pre-session state while the guard is open. A successful commit validates
the package, fully reparses the model, text, metadata, notes, comments, fields,
shapes, images, and renderer sidecars, and retains the original package object so
`edited_parts()` evidence is unchanged. Direct edit callers can perform the same
atomic refresh with `doc.refresh_read_view()?` instead of saving and reopening.
Refresh performs one in-memory DOCX serialization and parse; it is not an
undo/redo stack and cannot retract saved bytes or other external side effects
already used by the caller.

Every one of the edit methods above mutates live WordprocessingML **element
trees** or media parts in place, so everything they don't touch — including
content the lossy model can't represent (fields, content controls, shapes,
comments, tracked changes) — is structurally preserved; `save()` re-serializes
only the parts you changed, while untouched package-part payloads stay
byte-for-byte.
`body_blocks()` enumerates conservative atomic direct `w:p`, `w:tbl`, and
`w:sdt` subtrees. `insert_body_paragraph` inserts one unstyled direct paragraph
before an indexed block, or before final body `w:sectPr` at the block-count
position, while reusing the edit layer's WML whitespace, tab, line-break, Unicode,
and forbidden-control handling. Structural edits reject opaque direct children
and cross-block ranges or complex fields; move/remove also protect section-boundary
targets and moves. Rich paragraph/block insertion, nested containers, and
relationship-bearing content remain outside this bounded API. Existing exact
subtrees and untouched package parts are preserved, but removals do not
garbage-collect relationships or media they make unreachable. Read/model views
remain stale until explicitly refreshed or reopened.
Regenerated relationship parts are validated before save, so internal
relationship targets must point at retained package parts unless they are
explicitly external.
`Document::new()` starts from a bundled
blank template. To *author* a
document from data (or convert a `.doc`), build a `DocModel` and use
[`write_docx`](#write--author-a-styled-docx) instead.
Call `edit_capability()` or inspect `report().edit` before editing if you need
machine-readable read-only reasons such as legacy `.doc`, incomplete retained
packages, or lossy OPC metadata. Call `edited_parts()` after edits to inspect
the sorted package part names that will be reserialized or regenerated; the same
list is included in `report().edited_parts` and diagnostics JSON. Core metadata
from `core_properties()` is included in `report().core_properties`; parsed
string custom properties are included in `report().custom_properties`.

## Render — typeset to PDF

Lay any model out to a paginated **PDF** with native typesetting — `parley` shapes
and line-breaks (Korean/CJK [UAX #14] line-breaking + script font fallback),
`krilla` emits the PDF with subsetted embedded fonts and **selectable text**. Rich
runs (color/size/font, highlight, decorations, super/subscript, caps/small-caps),
paragraph shading, line spacing, explicit before/after spacing, first/hanging
indents, lists with
reader-captured autonumber labels or deterministic empty-label fallbacks in
top-level body and ordinary or recursively flattened table cells, bordered
tables with six-way model-backed solid colors and bounded
per-side eighth-point widths, shaded vertically-aligned cells with modeled
physical cell margins, authored and
opened-DOCX `tblGrid` column proportions, bounded preferred-percentage outer
widths, logical leading/center/trailing placement, and non-negative leading
indentation, top-level and run-attached body images with model-backed
whole-degree clockwise rotation, and **clickable hyperlink annotations** are
drawn. Opened `.docx` tables materialize direct `dxa`/`nil` `w:tblCellMar`
defaults and per-side direct `w:tcMar` exceptions, including logical
`start`/`end` mapping under `w:bidiVisual`, and inherit a table style's own
`w:tblCellMar` through its `w:basedOn` chain, plus its `wholeTable`
and bounded `band1Horz`/`band2Horz`/`firstRow`/`lastRow` conditional regions,
beneath those direct declarations. A row-local
`w:tblPrEx/w:tblCellMar` replaces the direct table margin property for that row,
inherits omitted sides from the style or schema defaults, and remains beneath a
direct cell declaration. Vertical-band, column, and corner conditional regions
remain outside this bounded subset. A table style's `w:tblBorders`, from its own
`w:tblPr` or its `wholeTable` region and resolved through the same chain, applies
when the table declares no borders of its own. A style's `w:tblW`, `w:tblInd`,
and table `w:jc` fill in only what the table itself leaves unset. Its
`wholeTable` and bounded row-region `w:tcPr` supply cell shading and vertical
alignment, and preserve percentage preferred width, for cells that declare
none; preview grid geometry still comes from table/grid widths rather than
per-cell preferred width. A style's
`w:tblLayout` and `w:bidiVisual` apply
when the table declares neither.
Rotated raster bounds drive proportional
content-box/page-height fitting and pagination. Narrow RTL tables reverse logical
placement and mirror their cells inside the local table box. Page geometry,
equal-width section columns, and per-side margins come from the document;
multi-page tables repeat their header rows without losing outer placement or
border paint. Opened
`.docx` rows, including recursively flattened nested rows, may split across
pages by default, while effective `w:cantSplit` from direct row properties or
an inherited table-style chain keeps a fitting row together and an over-tall
row still splits at line boundaries. Table styles
include direct non-conditional row properties and bounded
`wholeTable`/`band1Horz`/`band2Horz`/`firstRow`/`lastRow` conditional regions
selected by direct table `w:tblLook` or row `w:cnfStyle`. Inherited style and
direct-table `w:tblStyleRowBandSize` values use Word's 0-3 row grouping; later
regions and direct row formatting retain Word precedence. Model-only renders
retain the established keep-together default. Opened legacy `.doc` rows follow direct
`sprmTFCantSplit` and compatibility `sprmTFCantSplit90`: absent properties
retain the MS-DOC splittable default, the modern property takes precedence when
both are present, and fitting protected rows move whole while over-tall rows
still make deterministic progress. Emitted nonblank top-level and ordinary
table-cell paragraphs from opened legacy `.doc` files additionally resolve
paragraph-style STSH inheritance followed by direct PAPX for `sprmPFKeep`,
`sprmPFKeepFollow`, and default-on `sprmPFWidowControl` through private
source-aligned hints; resolved `sprmPFPageBreakBefore` maps to the existing
model property. Explicit direct-off values override inherited-on values.
Fitting protected content moves whole and over-tall content still splits for
deterministic progress. Opened `.docx` `Document` renders
additionally honor resolved left/center/right/decimal tab stops in LTR
left/start-aligned top-level body paragraphs, plus default and resolved
logical-start tab stops in RTL right/start-aligned top-level body paragraphs.
LTR stops retain their page-text-margin coordinates under supported left,
positive first-line, and hanging indents; RTL logical-start stops retain
leading-edge coordinates from the right page-text margin under resolved
physical or logical left/right indents. Default-tab fallback targets use the
same margin-anchored grid and are clamped to the active paragraph box. Opened
documents also honor document-default,
declared default paragraph-style, and bounded explicit paragraph-style
before/after spacing, proportional automatic line spacing, first-line/hanging
indents, flat RGB shading, and `pageBreakBefore`; authored zero/off direct
overrides; and source-aligned `keepNext`,
`keepLines`, and default-on `widowControl` pagination hints in top-level body
paragraphs and direct or accepted-current wrapper-contained paragraphs in
ordinary or recursively nested table cells, without adding those source-only
render hints to the public `DocModel`. Table-cell tab semantics, RTL
center/end/decimal tab stops, center/right/justified LTR paragraph alignment,
leaders/bar tabs, settings-defined default-tab intervals, and implicit
hanging-indent/list-marker tabs remain outside this bounded tab support. Within
the supported LTR and RTL paths, resolved tab advances now reserve line width
before breaking, so content that no longer fits after a tab moves to the next
line instead of running past the paragraph box; the reservation only ever
tightens and is bounded to three passes, keeping the result deterministic, and
emitted page counts follow the resulting line count. Post-tab field
containment, Word-exact custom-tab-aware reflow, and Word-exact tab-driven
pagination remain outside the claim. Nested table content remains
a flattened text preview rather than a nested grid layout.
One per-story fallback-counter state follows source-logical order across body,
direct-cell, nested-cell, and later body paragraphs. `w:bidiVisual` changes
physical cell placement without reversing numbering, and split rows or repeated
headers reuse the already-shaped marker.
Finite positive explicit paragraph before/after spacing on emitted paragraphs in
ordinary and recursively flattened table cells participates in row measurement,
row splitting, vertical alignment, repeated-header placement, and layout-page
reporting. Spacing is attached once to the retained paragraph edges, so bounded
line truncation does not invent a trailing edge; unset, zero, negative, or
non-finite values add no space. Spacing-only empty or hidden-only paragraphs
remain omitted when they produce no shaped line.
Opened legacy `.doc` tables with strictly increasing `sprmTDefTable` row
boundaries and common logical outer edges also retain normalized relative
column proportions, including mixed internal row grids represented by column
spans. Missing or inconsistent geometry retains content-sized columns.
Strict rectangular, unmerged tables with at least two rows and columns
additionally recover coherent positive top, left, bottom, right,
inside-horizontal, and inside-vertical borders from complete `TC80` records
plus later direct row-mark `sprmTTableBorders80` and `sprmTTableBorders`
operands when the embedded cell edges remain default/inheritable. Compatible
and modern direct values apply in source order, and physical sides remain
correct for visual RTL tables. Recovered colors, widths, and supported line
styles survive `.docx` conversion/reopen; PDF preview consumes the colors and
widths through its existing solid-stroke table paint. Mixed automatic/explicit
colors, nil/no-border roles, unsupported line effects, conflicting shared
edges, malformed operands, pre-definition row borders, topology-changing
modifiers, per-cell overrides, and incomplete or zero-width grids retain the
border fallback instead of projecting a misleading whole-table result. Valid
definitions with fewer or excess complete `TC80` records or equal boundaries
still preserve table structure.
Eligible front-of-text
`wrapTopAndBottom` shapes with explicit page/margin or enabled `simplePos`
vertical geometry also exclude later flow from their page-wide vertical band
after the recovered top-level paragraph anchor. Pagination-protected paragraphs
and keep-next chains retain their controls, and same-paragraph inline images do
not participate in the bounded exclusion. Behind the `render` feature.

Bounded RTL rendering applies `w:bidi` paragraph base direction, `w:rtl` run
isolation, logical alignment/list placement, and `w:bidiVisual` table column
mirroring. This improves mixed Arabic/Hebrew documents without claiming
Word-exact list-level alignment, punctuation, or table typography.
`ListInfo` does not retain list-instance identity, source `numId`/`ilfo`,
restart/start overrides, marker fonts or glyph metadata, or marker
tabs/alignment/exact hanging indents. Independent or restarted empty-label
lists therefore use deterministic preview numbering rather than Word-exact
numbering. Marker-aware table autofit, nested-grid geometry, table-cell images,
legacy nested-table recovery, and Word-exact RTL list typography remain outside
this preview.
Opened legacy `.doc` runs additionally preserve literal direct
`sprmCFBiDi` on/off values from complete FKP/CHPX payloads through `.docx`
conversion and PDF run isolation. Opened legacy paragraphs preserve valid
direct `sprmPFBiDi` direction together with physical left/center/right and
logical start/center/end alignment from `sprmPJc80` and `sprmPJc` through
`.docx` conversion/reopen and existing PDF body/table-cell shaping; paragraph
styles additionally resolve this bounded direction/justification subset
through cycle-guarded, depth-bounded STSH inheritance before final direct PAPX
overrides. Modern logical `sprmPDxaLeft`, `sprmPDxaRight`, and
`sprmPDxaLeft1` twip indents are also retained from direct PAPX and paragraph
styles: sparse style values resolve through the same bounded base chain before
final direct overrides, logical edges resolve against final paragraph
direction, and a negative first-line offset becomes a hanging indent. Direct
`sprmPNest` is additive when a style-derived or direct logical-left base exists;
the prohibited style form and direct nest-only values without a base remain
unmaterialized. Valid `sprmPDyaBefore` and `sprmPDyaAfter` unsigned-twip
paragraph spacing plus positive proportional `sprmPDyaLine` LSPD values resolve
through the same style inheritance and final direct-PAPX precedence. Omitted
values materialize the MS-DOC defaults of zero points before/after and single
line spacing; supported values survive shared-model use, `.docx`
conversion/reopen, and top-level or table-cell PDF preview layout.
Direct and paragraph-style `sprmPShd80` and `sprmPShd` paragraph shading also
reaches the shared model, `.docx` conversion/reopen, and PDF preview when the
source result collapses exactly to one explicit RGB fill: clear uses its
background, solid uses its foreground, and other supported patterns require
identical explicit foreground/background colors. Style-local values resolve
through the bounded base chain before final direct-PAPX precedence. Later
structurally complete automatic, nil, patterned, invalid, or wrong-sized values
suppress inherited or earlier positive state, and a later valid value recovers.
A truncated or unsizeable direct shading modifier suppresses the effective
fill and stops that PAPX scan; a structurally malformed style UPX invalidates
its local style payload. A later paragraph-style modifier resets earlier
direct shading.
At-least/exact and explicit
zero proportional LSPD values clear an inherited multiplier but remain unset
because the shared model has no corresponding line-rule representation.
Paragraph direction does not imply table mirroring. Opened
legacy tables preserve strict direct row-mark
`sprmTFBiDi` and compatibility
`sprmTFBiDi90` Bool16 values: either final property enables visual RTL,
direction changes split adjacent rows into separate tables, and cells remain
in source-logical order for `.docx` conversion and PDF column mirroring.
Distribution and language-specific justification collapse to generic justify.
Character-style or language-derived direction, table-style-derived direction,
indented logical justification, list-level, compatibility-era, character-unit,
and mirrored legacy indents, line-unit/auto/contextual paragraph spacing,
Word-exact adjacent-spacing resolution, table/list-style spacing effects, exact
RTL list-level layout, patterned shading fidelity, theme/automatic/nil color
distinction, document-default or table/list-style conditional shading, original
legacy style-graph preservation through `.docx` conversion,
negative-indent PDF outdenting, piece `Pcd.Prm` paragraph
direction/modifiers, and Markdown/HTML visual RTL remain outside these bounded
bridges.

> **Scope:** this is a fast, in-process **preview / report** renderer, not a Word
> layout engine. It is faithful to the model and produces selectable text, but
> does **not** claim Word- or LibreOffice-exact pagination, floating-object
> layout, end-to-end RTL typography, page-bottom footnote composition, unequal
> section columns, or section-local page geometry. Unknown fields, remaining
> layout-dependent TOC/REF/NOTEREF cases, and unsupported value-changing field
> semantics retain their cached display text with diagnostics.
> Conditional table-style vertical bands, first/last-column and corner regions,
> and `w:tblPrEx` row-group exceptions do not yet contribute `cantSplit`.
> Exact/at-least line rules, nonzero line-unit before/after spacing,
> enabled automatic/contextual paragraph spacing, nonzero character-unit indents,
> theme/automatic/pattern paragraph shading, and table/list/conditional-style
> paragraph properties remain outside the bounded style-derived layout subset.
> Nested-table paragraph and row controls retain the renderer's 32-level
> flattening bound; nested grid/border geometry and Word-exact cell spacing/tab
> geometry remain outside this slice. Legacy STSH properties beyond the bounded
> paragraph-pagination,
> direction/justification, modern logical-indent, and paragraph-spacing
> subsets,
> table/list-style paragraph effects, piece `Pcd.Prm` paragraph properties and
> character forms beyond the bounded literal `Prm0`/`Prm1` subset described
> below, nested legacy
> tables/rows, and controls attached only to discarded blank top-level
> paragraphs remain unsupported. Legacy absolute table width, autofit,
> indentation, preferred cell-width modifiers, row-specific outer-edge
> geometry, table-style-derived RTL, and additional table-boundary properties
> remain outside the bounded direct-property bridges. PDF table placement treats
> a finite positive relative width as a preferred width within the active page or
> section column and bounds leading indentation to the remaining horizontal
> space. A complete positive direct DOCX `tblGrid` supplies relative model column
> proportions only when its count matches the reconstructed cell/span grid;
> malformed, revision-history, excessive, and mismatched grids keep the
> content-sized fallback. Absolute/auto table widths, preferred-cell/table/grid
> conflict resolution, a Word-exact fixed/autofit algorithm split, table-style
> and `tblPrEx` placement, floating or nested-grid placement, negative outdents,
> and table `both` justification retain deterministic fallbacks. PDF
> table-border paint resolves physical top, left, bottom, right,
> inside-horizontal, and inside-vertical colors and widths from side-specific
> model values before uniform and black/0.4-point fallbacks. Widths are capped at
> 12 points, then conservatively bounded across the table by its smallest laid-out
> cell. Physical sides remain attached after visual RTL placement, modeled
> row/column spans suppress covered inside edges, and split row fragments omit
> artificial horizontal seams.
> Non-solid/none styles, cell borders, theme/style inheritance, cell spacing and
> border-conflict resolution, ragged-row repair, nested-grid borders, and
> legacy `.doc` per-cell, no-border, conflicting, merged/ragged, nested, or
> style-derived border recovery remain outside this bounded renderer slice.
> PDF image rotation normalizes direct-model angles modulo 360 and rotates the
> decoded raster around its center. Source-authored display extents, crop/flip/
> effects, floating-anchor offsets, exclusion-zone reflow, table-cell images,
> and Word-exact inline baseline placement remain outside this bounded image
> bridge.
>
> Opened-document renders draw bounded approximate overlay boxes for recovered
> `.docx` floating-shape geometry on the recovered top-level body block page. A
> forward `wrapTopAndBottom` subset moves eligible post-anchor lines and later
> block images/charts below page-wide rectangular exclusions; backward,
> same-paragraph image, table, polygon, and Word-exact wrap reflow remain out of
> scope. Compact placeholder lines represent preserved charts, OLE objects,
> unsupported or composed WMF/EMF/EMZ/WMZ payloads, unavailable image bytes,
> backend-incompatible raster images, and floating shapes without recoverable
> geometry. Exact single-DIB WMF/EMF raster streams are decoded separately.
>
> Measured against LibreOffice on the public corpus it reaches 0.996 mean text
> recall with matching page counts; for archival or Word-exact PDF, render via
> LibreOffice.
> (See *Scope & parity*.)

```toml
rwml = { version = "0.1", features = ["render"] }
```

```rust
let pdf = rwml::render_pdf(&model);                 // uses system fonts
let pdf = rwml::try_render_pdf(&model)?;             // fallible variant
// On a headless host without CJK fonts, supply your own:
let kr  = std::fs::read("NotoSansKR-Regular.otf")?;
let pdf = rwml::render_pdf_with_fonts(&model, &[kr]);

let rendered = rwml::render_pdf_with_report(&model);
eprintln!(
    "pages={} render_warnings={}",
    rendered.report.pages,
    rendered.report.warnings.len()
);
```

For portable Korean, Arabic, and Hebrew PDF rendering, enable `bundled-fonts`.
The separate OFL-licensed `rwml-fonts` crate registers Noto Sans subsets for KS
X 1001 Hangul and hanja plus Latin, Arabic, and Hebrew, including the OpenType
tables required for Arabic/Hebrew shaping. The main `rwml` crate remains
MIT-licensed. Scripts outside those subsets continue through the normal system
or caller-supplied font fallback used by `render_pdf_with_fonts`.

`layout_pages_with_fonts` exposes **layout-derived page numbers** from the same
preview-grade pagination: the page count plus the page each body `PAGE` field
and top-level block lands on — matching rwml's own PDF output, **not**
Microsoft Word's pagination. Supplied fonts are used strictly (system fonts
disabled), so identical document + font bytes give identical pages; results
live in a separate `LayoutPages` record and never overwrite reader-path
`computed_result` semantics. Modeled next/even/odd section breaks advance at
least one physical page; even/odd starts add one body-empty filler when needed
to reach the requested 1-based physical parity. Section display-number
restarts/formats do not affect that preview parity. Word-exact filler-page
running surfaces, section-relative odd/even header selection, and mixed
section-local geometry remain outside this bounded behavior.

You can also convert a parsed document straight to PDF:
`Document::open(&bytes)?.to_pdf()` / `try_to_pdf()`, pass font blobs with
`to_pdf_with_fonts()` / `try_to_pdf_with_fonts()`, or use
`to_pdf_with_report()` / `to_pdf_with_fonts_and_report()` when you want page
count and renderer warnings tied to the opened document's feature inventory. The
opened-document paths use that inventory for visible placeholder lines when
unsupported preserved objects are present.

## CLI (examples)

```text
rwml extract  file.docx                                # plain text
rwml convert  file.doc md                              # Markdown / html / txt
rwml diagnose file.docx                                # JSON feature report
rwml to-docx legacy.doc out.docx                       # .doc → clean .docx
rwml to-pdf file.docx out.pdf --report-json render.json # PDF + render report

cargo run --bin rwml -- diagnose file.docx             # same CLI from source
cargo run --features render --bin rwml -- to-pdf file.docx out.pdf --report-json render.json
cargo run --example report   -- report.docx            # author a styled report
cargo run --features render --example to_pdf -- file.docx out.pdf
cargo run --features render --example to_pdf -- file.docx out.pdf --report-json render.json
python scripts/render_validate.py --json --page-cap 32 --min-mean-recall 0.90 --max-skipped 0 corpus/public/**/*.docx > render.json
# `--verify-oracle` re-renders every reference to check the oracle reproduces
# itself. Visual metrics are only comparable across runs when it reports true,
# which needs a host with no duplicate font families: two builds sharing one
# family name but differing in vertical metrics make LibreOffice pick between
# them per run, shifting every baseline.
VERSION=0.1.2
REV="$(git rev-parse HEAD)"
python scripts/bench_vs_mature.py --corpus corpus/public/benchmark --json \
  --version "$VERSION" --git-rev "$REV" \
  --min-poi-recall-mean 0.95 --min-poi-f1-mean 0.95 --max-errors 0 --min-scored 1 \
  --output dist/extract-benchmark.json
python scripts/public_hygiene_audit.py --json > dist/public-hygiene.json
python scripts/release_manifest.py --version "$VERSION" --git-rev "$REV" \
  --release-policy public-release \
  --enforce-policy-inputs \
  --hygiene-report dist/public-hygiene.json \
  --corpus-manifest corpus/public/MANIFEST.tsv --corpus-manifest corpus/public/RENDER_MANIFEST.tsv \
  --validation-report render.json --benchmark-report dist/extract-benchmark.json \
  --output dist/rwml-release-manifest.json dist/*
```

## Cargo features

| feature | default | pulls in | enables |
|---|:--:|---|---|
| `docx`   | ✅ | `zip`, `quick-xml`, `flate2` | `.docx` read, `write_docx`, **and package-preserving edit/`save`** |
| `render` |    | `parley`, `krilla` | `render_pdf` / `to_pdf` (MSRV 1.92) |
| `bundled-fonts` |    | `render`, `rwml-fonts` | `render_pdf_bundled` with OFL Noto Sans subsets for KS X 1001 Korean + hanja, Arabic, and Hebrew |

The library also emits an `rlib` plus `cdylib`; on `wasm32` it uses a
target-specific `wasm-bindgen` dependency for the thin `rwml::wasm` read/report
adapter (`extractText`, `markdown`, `html`, `reportJson`).
[`examples/wasm-demo`](examples/wasm-demo) is a static browser inspector over
that adapter: it opens local files, shows text/Markdown/HTML preview, and exposes
the same diagnostics JSON without adding an editing UI.

For a dependency-light, legacy-only build (just `cfb` + `encoding_rs` +
`thiserror`): `rwml = { version = "0.1", default-features = false }` (reads `.doc`,
emits text/markdown/html).

## Why one crate? (and how this relates to `docx-rs`)

The mature [`docx-rs`](https://crates.io/crates/docx-rs) proves there is real
demand for Rust-native `.docx` authoring. `rwml` aims higher than a writer-only
surface: legacy `.doc` (no comparable pure-Rust option exists) and modern `.docx`
produce the *identical* [`DocModel`] and share one read/write/edit/render/report
surface, with no JVM, no subprocess, and no second Word parser in the tree.

## How it works

A `.docx` is a ZIP of XML parts. `rwml` reads `word/document.xml` with `quick-xml`
by recursive descent (paragraphs → runs with `w:rPr`; tables `w:tbl` with
`gridSpan`/`vMerge` → real colspan/rowspan), resolves heading levels from
`word/styles.xml` (`w:pStyle` / `Heading N` / `제목 N`), supported
document-default/paragraph/character run properties, and bounded paragraph
style layout values; ordered-vs-bullet from `word/numbering.xml`; and hyperlink
targets + image bytes from
`word/_rels/document.xml.rels` + `word/media/*`. Running headers/footers are
resolved from the `sectPr` references (`word/header*.xml` / `footer*.xml`, each
with its own rels) into section-break setup plus the final `DocSetup`, including
default, first-page, and even-page variants where present, and text-box text
(`w:txbxContent`, DrawingML or VML, single-branch on `mc:AlternateContent`) is
folded back into the body.
Recursion is depth-capped, XML external entities are never resolved (XXE-safe), and
per-entry decompression is size-capped (zip-bomb guard).

`.doc` is an OLE2 compound file. `rwml` opens it with `cfb`, parses the **FIB** by
*navigating* its variable-length sub-structures (never hard-coded offsets) to the
piece table and sub-document char counts, decodes each piece as UTF-16LE or 8-bit
text in the document's ANSI codepage derived from the FIB language id (`lid`) — so
Korean (`0x0412` → cp949), Japanese, Cyrillic, etc. decode correctly. The **rich
model** is a lazy second pass: the CHPX character-property bins (bold/italic/
underline/strike, **font name from `SttbfFfn`, half-point size, color, and
CHPX-resident `sprmCHighlight` palette highlighting plus direct `sprmCIss`
super/subscript plus literal direct `sprmCFSmallCaps`/`sprmCFCaps`
capitalization**), the STSH style sheet + outline levels (headings),
`sprmTDefTable` (merge-aware tables with bounded relative column proportions),
`PlcfSed`/SED section boundaries with SEPX page size, orientation, nonnegative
per-side margins, title-page state, and page-number restart/format state,
`PlfLst` list autonumbers with bounded `PlfLfo`/`LFOLVL` per-instance overrides,
restart thresholds, legal-number formatting, and shared-list continuation,
hyperlink field marks, and
`PICF` inline images. The rich pass also retains each piece's PCD `Prm` and
ordered CLX PRCs. After CHPX, it applies literal off/on `Prm0` values for bold,
italic, strike, small caps, caps, and hidden text, plus precompiled bounded
`Prm1` values for those properties, underline, run RTL, highlight palette
colors, and baseline/superscript/subscript. Supported `Prm1` values are
source-order stable with explicit clears, underline styles collapse to the
shared boolean model, and each validated group is scanned once during open.
These piece modifiers flow through `.docx` conversion; supported visible
properties also reach the existing PDF path. Missing, malformed,
style/reset-dependent, and style-relative groups remain inert. Piece-level
font/size/color and complex-script effects, pictures/OLE,
paragraph/list/table/section properties, tab changes, revision-original
formatting, full character-style resolution, arbitrary `sprmCHpsPos` shifts,
and visual highlight/super/subscript/capitalization preservation in
Markdown/HTML remain outside this path.

The `.docx` **writer** is the inverse of the reader, part by part: `document.xml`
(`w:rPr`/`w:pPr` with the full property set), a synthesized `styles.xml`
(Normal + Heading1–6 with `outlineLvl`), `numbering.xml`, header/footer parts wired
through `sectPr`, media parts + relationships for images, and external relationships
for hyperlinks. The **renderer** flows the model through its authored page geometry
and section columns, then draws each page's glyph runs, table grids, shading, and
images with krilla.

Encrypted / XOR-obfuscated documents and pre-Word-97 (Word 6/95) files are detected
and reported as distinct [`Error`]s rather than silently emitting garbage. Every
read is bounds-checked: malformed input returns an [`Error`], never a panic — safe
to run untrusted files in-process (`#![forbid(unsafe_code)]`, fuzzed).

## Scope & parity

**Reading.** Flat text targets POI `WordExtractor.getText()`. `.doc` validated on
private Korean-language `.doc` fixtures at ~97.4% whitespace-insensitive parity
vs POI (body text ~100%). `.docx` validated against
**python-docx** on the 127-file Apache POI corpus (87 with comparable text):
**98.6% mean word recall, 100% median, 85/87 ≥ 99%**, whole corpus panic-free
(incl. clusterfuzz inputs). The two sub-99% files are a duplicate-`document.xml`
zip-confusion container and a heavy tracked-changes doc (accept-changes view) —
explained, not extraction bugs.

**Writing.** `read → model → write_docx → read` round-trips structure and the rich
character/paragraph/table formatting (covered by unit + integration tests); the
authored report opens in Word and LibreOffice.

**Editing (package-preserving).** `Document::open` retains the whole package and
`save()` re-emits it with every unmodeled part preserved verbatim — a no-op
open→save is **part-payload** byte-stable (the ZIP container metadata is normalized).
Edits go through live WordprocessingML **element trees** or media-part
replacement (the methods shown under
[Edit — open, change, save](#edit--open-change-save-package-preserving)), so
unmodeled body content (fields, shapes, content controls, comments, tracked
changes) survives.
`edited_parts()` reports touched package parts, and `Document::report()` includes
core metadata, edit capability, and edited part names; it emits
`PackageReadOnly` when preservation edits are refused.
Validated on the 127-file corpus with python-docx
as the strict external checker: passthrough is part-payload byte-stable; the
element-tree image insert produces a package python-docx opens with the inline image
present on every openable file; both fail cleanly (no panic) on a pathologically-deep
file and a structurally-broken original. To author/convert from a `DocModel`, use
`write_docx` (it regenerates a fresh package, lossy w.r.t. unmodeled content).

**Rendering.** [`scripts/render_validate.py`](scripts/render_validate.py) compares
the renderer to LibreOffice per document using text recall, page-count ratio, the
historical page-1 72-DPI aHash, all-matched-page aHash, foreground ink IoU, and
explicit unmatched/capped page counts, plus rwml render-warning counts/kinds.
The multi-page metrics report their DPI, foreground threshold, hash size, page
cap, and fixed-bundled-font mode; page canvases are white-padded without scaling
and raster pairs are processed under hard pixel limits. Its `--soffice auto`
default uses a local `soffice` when available and otherwise falls back to the
Docker `lo-cli` backend. The public synthetic corpus also includes a render
manifest checked by `cargo test --features render`. rwml is today a **preview-grade**
renderer, faithful to the model but **not yet** a LibreOffice replacement. Generated
running footer page numbers and body `PAGE` fields are computed from the emitted
PDF page list; section-aware default/first/even running header/footer variants
are selected with first-page variants scoped to each section and even variants
based on emitted page parity; field-code `HYPERLINK` runs render as link
annotations for target/anchor, tooltip/frame, and documented `\m`/`\n` no-op
switch tails, and malformed hyperlink syntax reports `UnsupportedSwitch`.

**Field evaluation** is deterministic and source-order stable. The reader,
render model, and side-table text surfaces (comment bodies/anchors,
tracked-change text, note anchors, floating-shape and text-box text, TOC heading
sources) share the same evaluators when the scanner has the field family's
required source context; otherwise cached result text is preserved. Every
`FieldKind` is distinguished from an unknown field; where a value can't be
computed deterministically the cached result text is preserved (including
inline tabs, line breaks, and no-break/soft hyphens for simple and common
complex fields) and a reason is reported. The four
diagnostic reasons — `UnknownField`, `UnresolvedBookmark`, `UnsupportedSwitch`,
`NoComputedResult` — are surfaced with both field-kind counts and reason counts,
and malformed instruction syntax for any supported family reports
`UnsupportedSwitch`.

| Family (fields) | Computed subset | Cached / ceiling |
|---|---|---|
| **Document-info / date / stat** (`AUTHOR`, `TITLE`, `SUBJECT`, `KEYWORDS`, `COMMENTS`, `LASTSAVEDBY`, `CATEGORY`, `VERSION`, `NUMPAGES`, `NUMWORDS`, `NUMCHARS`, `EDITTIME`, `TEMPLATE`, `FILESIZE`, `CREATEDATE`, `SAVEDATE`, `PRINTDATE`, `DOCPROPERTY`, `DOCVARIABLE`, `INFO`, …) | Metadata-backed values from `docProps/core.xml` / `custom.xml` / `app.xml` and `word/settings.xml`, with simple numeric `\@` date pictures (`y`/`M`/`d`/`H`/`h`/`m`/`s`, English `MMM`/`MMMM`, `ddd`/`dddd`, `AM/PM`), `\*` number formats, and `FILESIZE` `\k`/`\m` switches; direct `USERNAME`/`USERINITIALS`/`USERADDRESS` literal overrides | Cached date/user/unmapped fields render warning-free when syntax is valid |
| **Formula / expression** (`=`, `IF`, `QUOTE`, `COMPARE`, `FILLIN`, `ASK`, `SET`, `NEXT`, `NEXTIF`, `SKIPIF`) | Literal arithmetic (`+ - * / ^`, parens, unary), scalar functions (`ABS`, `AND`, `AVERAGE`, `COUNT`, `DEFINED`, `IF`, `INT`, `MAX`, `MIN`, `MOD`, `NOT`, `OR`, `PRODUCT`, `ROUND`, `SIGN`, `SUM`, `TRUE`/`FALSE`), comparisons, non-spanning table-position formulas (A1/RnCn/`LEFT`/`RIGHT`/`ABOVE`/`BELOW`), literal `QUOTE`/`IF`/`COMPARE` (incl. `?`/`*` wildcards), `FILLIN`/`ASK` default responses, `SET` bookmark assignments feeding later `REF`/comparisons; `\#` numeric pictures and neutral/text-format tails | Bookmark/reference formula expressions, broader picture syntax, and non-literal forms report `NoComputedResult` |
| **PAGE** | Current page from trusted leading structural / source-rendered context, section `w:pgNumType` restarts + supported page-number format styles, page-number and field-result format switches | Broader layout-derived current-page cases keep cached text |
| **PAGEREF** | Page numbers from leading page breaks / `pageBreakBefore` / section starts, restart labels + supported `w:fmt` styles, `\*` number formats, `\p` relative (`above`/`below`/`on page N`) | Remaining layout-dependent references keep cached text; missing targets → `UnresolvedBookmark` |
| **REF / direct bookmark** | Bookmark text (incl. hidden targets, multi-paragraph ranges), `\* Upper/Lower/Caps/FirstCap`, `\#` numeric picture on numeric bookmark text, `\p` relative, numbered-paragraph `\n`/`\r`/`\w` (+ `\p`/`\t`), `\f` note-reference marks, neutral `\h`/`\!`, text-neutral `\d "sep"` | Value-changing `\d` separators, non-numeric `\#` targets, and broader REF semantics keep cached text |
| **NOTEREF / FTNREF** | Footnote/endnote reference marks (honoring `settings.xml` `numStart`/`numFmt` and skipping `w:customMarkFollows` auto-numbering), `\h`, `\f`, `\p` above/below, number/text format switches | Missing targets → `UnresolvedBookmark`; no note mark or custom-mark target → `NoComputedResult`; per-page note restart is layout-dependent |
| **STYLEREF** | Nearest styled paragraph/run text by style id or name (backward-then-forward), `\p` above/below, numbered `\n`/`\r`/`\w`/`\t` | Page-aware / header-footer / layout-dependent lookup keeps cached text |
| **TOC / TC / SEQ** | Default, `\b` bookmark-scoped, `\o`/`\u` outline, `\t` custom-style, `\f` from `TC` markers, `\c`/`\a` caption entries from `SEQ`; source-order `SEQ` recompute; `\h`/`\z`/`\w`/`\x`/`\n`/`\p`/`\s`/`\d` and `\*` switches | Advanced/layout-dependent TOC cases keep cached text; missing `\b` scope → `UnresolvedBookmark` |
| **SECTION / SECTIONPAGES / REVNUM** | Current structural section number; structurally bounded section page counts; `REVNUM` from `cp:revision`; page-number and field-result format switches | Layout-dependent section page counts keep cached text |
| **Display / layout** (`EQ`, `SYMBOL`, `ADVANCE`) | `EQ` fractions/radicals/brackets/boxes/lists/arrays/scripts/integrals/overstrikes as plain text; `SYMBOL` decimal/hex/ANSI/`\u`/font `\f`/size `\s` incl. common Symbol/Wingdings; `ADVANCE` hidden with validated movement switches | Layout offsets, broader equation formatting, and rarer font mappings keep cached text |
| **Numbering / list** (`AUTONUM`, `AUTONUMLGL`, `AUTONUMOUT`, `BIDIOUTLINE`, `LISTNUM`) | Source-order counter values with common number/text formats and `\s` separators/starts; level-1 `LISTNUM NumberDefault`/`LegalDefault` | Richer outline semantics and named/multi-level lists keep cached text |
| **Legacy form** (`FORMTEXT`, `FORMCHECKBOX`, `FORMDROPDOWN`) | `w:ffData` checkbox checked/default states, dropdown result/default selections, non-empty text-input current results or empty-current `w:textInput` defaults | Broader protected-form behavior keeps cached text |
| **Action / automation** (`GOTOBUTTON`, `MACROBUTTON`, `PRINT`) | Display text without executing navigation/macros; `PRINT` printer-control groups render hidden without executing instructions | Broader forms keep cached text |
| **Inserted content, mail-merge helpers, reference/index, compatibility/private, barcode** | Named separately from unknown fields; simple literal `RD`/`TA`/`XE` markers render hidden | Cached text preserved; linked/external/opaque payloads not evaluated → `NoComputedResult` |

Volatile `DATE`/`TIME` (explicit `\@` picture) and `USERNAME`-family fields are
computed deterministically by supplying caller inputs via
`FieldContext`/`fields_with_context` — the context is an input, so identical
document plus identical context always yields identical results.

Authored charts render as native vector preview charts (see
[chart families](#chart-families)). On a real
`.docx` corpus it reaches **0.996 mean text recall** with a **1.00 mean page-count
ratio** (extracting headers/footers,
text boxes, nested tables, real list labels, caps; model-driven page geometry makes
`.doc` page counts line up). 23 of the 24 public-corpus documents score exactly
1.00; the exception is a right-to-left list fixture, for the reason given under
[Scope & parity](#scope--parity). It still trails
LibreOffice on exact pagination, exact floating-object layout, remaining
layout-derived `PAGEREF` page-reference computation beyond trusted source markers,
advanced TOC/REF/NOTEREF computed fields, and
pixel-level visual fidelity. Those gaps describe the renderer as it stands
today; closing them is the layout work tracked under [Roadmap](#roadmap), not
scope that has been ruled out. Until it lands, render via LibreOffice for
Word-exact or archival PDF. rwml aims to match specialist extractors on
text/model recovery while staying dependency-light; render fidelity remains
below LibreOffice.
`render_pdf_with_report` / `to_pdf_with_report` expose the emitted page count and
renderer warnings for unsupported fields, floating shapes, charts, OLE objects,
WMF/EMF/EMZ/WMZ images, image nodes whose bytes are unavailable, and raster
images skipped because the PDF backend cannot decode their bytes.
`Document::report().features.metafiles` exposes
metafile part path, format, stored byte size, compression flag, and header-derived
dimensions when a raw or gzip-wrapped EMF header or placeable WMF header makes
that cheap to recover. Floating-shape feature counts use the same accepted/current
revision and single-branch `mc:AlternateContent` policies as `floating_shapes()`,
so direct, inserted, and moved-to shapes count, deleted and moved-from old-only
anchors or markers are omitted, Choice/Fallback serializations of one shape
count once, and unrecovered alternate-content shape markers still count as one
marker.
Opened-document PDF rendering draws approximate overlay boxes for recovered
`.docx` `wp:anchor` geometry and anchor layout metadata,
including enabled `wp:simplePos` absolute placement, relative z-order,
behind/in-front flags, anchor `dist*` margins, `wp:effectExtent` bounds,
wrap-element `dist*` margins, wrap policy, `wp:wrapPolygon` point counts, and a
best-effort visible top-level body block anchor page, including body blocks wrapped by transparent content
controls, custom XML, smart tags, single-branch `mc:AlternateContent`, or
accepted/current revision wrappers, while omitting deleted and moved-from
old-only shape anchors. It
surfaces recovered containing-block anchor text, zero-width anchor character offsets inside that text, DrawingML preset geometry
names, simple sRGB solid fill/outline colors, anchor-distance and wrap-distance labels, wrap-polygon point counts, and
text-bearing shape body text in preview labels.
It appends compact placeholder lines for
preserved-but-unmodeled chart parts, OLE objects, unsupported metafile images,
image nodes whose bytes are unavailable, skipped raster images whose bytes the
PDF backend cannot decode, and shape markers without recovered geometry. Exact
body-page anchoring beyond that best-effort block page, text-wrap reflow beyond
bounded forward `wrapTopAndBottom` for explicit page/margin or enabled
`simplePos` geometry, and non-text Office-Art drawing contents remain out of
scope.
[`scripts/bench_vs_mature.py`](scripts/bench_vs_mature.py) emits a schema-tagged
JSON extraction benchmark report against local Apache POI and LibreOffice
goldens and can enforce release thresholds for mean POI recall/F1, mean
LibreOffice recall, scored-file counts, and extractor errors. Render-validation
JSON also carries a compact `gate` section for per-document recall plus optional
mean recall, page-ratio, legacy/all-page aHash, foreground-IoU, unmatched-page,
warning, and skipped-file thresholds. Release
manifests embed public corpus manifest totals plus public hygiene,
render-validation, and benchmark summaries/gates without copying row data, plus
the named `public-release` policy: required public hygiene audit,
fmt/clippy/default/no-default/render test gates, and selected optional local thresholds
(`0.95` POI recall/F1, at least one scored benchmark file, `0.90` render mean recall,
`0` extractor errors/skips).
Corpus TSV summaries reject empty manifests, duplicate columns or paths,
negative numeric counts, and duplicate warning tokens before embedding totals.
The same manifest records a compact `release_evidence` section so consumers can
tell whether strict local evidence was enforced, whether existing valid inputs
were provided without enforcement, and which strict public-release inputs are
still missing; public corpus evidence is incomplete when the manifests are
missing, invalid, their document path lists do not match, or their listed
documents are absent.
The hygiene audit covers normal text files, bounded decoded byte text views from
legacy `.doc` files, and Office package member paths plus textual parts from
`.docx`, `.xlsx`, and related OPC packages, including internal names, core
metadata, relationships, content types, WordprocessingML XML, and embedded
Office package XML such as chart workbooks, while leaving binary media payloads
opaque. Oversized legacy binary documents block the audit instead of passing
uninspected.
Add `--enforce-policy-inputs` when generating a strict public manifest from local
evidence: the command then requires a passing public hygiene report, render
validation, an `rwml.benchmark-report.v1` / `extract-vs-mature` extraction
benchmark, and exactly the public `MANIFEST.tsv` plus `RENDER_MANIFEST.tsv`
corpus manifests with matching document paths whose listed documents exist, and
rejects hygiene, validation, or benchmark reports whose compact gates failed or
were generated with weaker thresholds than the named `public-release` policy.
The release workflow intentionally emits the non-strict policy manifest from the
packaged `.crate` artifact, public hygiene report, and public corpus manifests,
then uploads the manifest and crate package as workflow artifacts before
publishing.
The renderer also maps a small common Symbol/Wingdings display subset to Unicode,
including the Symbol `0xB7` bullet, before PDF shaping; text extraction and exporters still preserve the source
code points.

**Still out of scope:**

- *Both formats (read/render):* OLE-embedded objects and exact floating Office-Art
  layout (`.docx` `wp:anchor` geometry, z-order
  metadata, enabled `wp:simplePos` absolute points, `wp:effectExtent` visual
  bounds, anchor `dist*` margins, wrap-element `dist*` margins, wrap policy,
  best-effort visible top-level body block page including transparent body
  content-control, custom XML, smart-tag, single-branch `mc:AlternateContent`,
  and accepted/current revision wrappers, omitting deleted and moved-from
  old-only shape anchors,
  containing-block anchor text plus zero-width anchor character
  offsets, DrawingML preset geometry names, simple sRGB solid fill/outline
  colors, and text-bearing shape body text are exposed through
  `floating_shapes()` and rendered as approximate preview overlays, not
  Word-exact anchored/wrapped Office-Art content. The renderer applies bounded
  forward vertical exclusion for eligible top-level paragraph
  `wrapTopAndBottom` anchors positioned from the page, page-margin text
  rectangle, or physical top/bottom margin bands, and overlay coordinates also
  distinguish physical left/right margin bands. Backward reflow, tables,
  parity-dependent inside/outside margins, character/column/line/paragraph
  coordinates, and square/tight/through/polygon wrapping are not implemented yet;
  metafile metadata is exposed
  in diagnostics with bounded header inflation, and a strict single full-frame
  DIB (`BI_RGB` 1/4/8/24/32-bit or 16/32-bit `BI_BITFIELDS`) in an exact
  header/raster/EOF stream with a frame-covering identity
  `STRETCHDIB`/`STRETCHDIBITS` or full-scan `SETDIBTODEV`/`SETDIBITSTODEVICE`
  record is extracted and rendered as a raster image. Source-bearing
  `EMR_BITBLT`, `EMR_STRETCHBLT`, `META_DIBBITBLT`, and
  `META_DIBSTRETCHBLT` records are also extracted as RGBA rasters and rendered
  only for exact `SRCCOPY`, zero source origins, positive one-to-one full-frame
  dimensions, canonical contiguous DIB payloads, and an exact identity EMF
  source transform using `DIB_RGB_COLORS`. Source-less WMF forms, 1-bit EMF
  source blits, scaling, cropping, mirroring, additional drawing records,
  composed raster operations, and general vector payloads are not rendered);
  unknown or broader fields' *computed* values
  (cached result text is kept, including inline tabs, line breaks, and
  no-break/soft hyphens for simple and common complex body fields; `.docx`
  REF/TOC cases listed above plus recovered `.doc` field instructions are
  exposed through `fields()`/diagnostics);
  complete symbol-font (Symbol/Wingdings) glyph coverage beyond the common
  deterministic mapped subset; encrypted files
  (detected and rejected).
- *`.doc` read only:* exact Valid Selection/story boundaries for shared-`lsid`
  list continuation, list-level PAPX/CHPX/style application, Word-exact list
  indentation/typography, and Word 6/95.
  Header, footnote/endnote, annotation, and text-box text appears in `text()` and
  dedicated region text APIs backed by `DocModel::regions`, with
  `DocModel::source_region_kind_text()` available for model-level region text.
  Non-empty annotation regions are exposed through `comments()` as best-effort
  recovered comments with source-region anchors, and footnote/endnote regions
  are exposed through `notes()` as best-effort recovered note records. A single
  unambiguous legacy footnote or endnote marker anchors to its containing body
  text; broader ambiguous note/endnote cases keep source-region anchors.
  Text-box regions are exposed through `text_boxes()` as best-effort recovered
  text-box records with source-region anchors.
  Header/footer regions are exposed through `header_footers()` as best-effort
  recovered records; when legacy `PlcfHdd` story boundaries are available, rwml
  splits stories and classifies exact even-page, odd-page, and first-page
  header/footer variants, otherwise it falls back to `Unknown` kind.
  `DocSetup` mirrors the first recovered default, even-page, and first-page
  legacy header/footer variants when story indexes are available, and falls back
  to a default running header for unsplit recovered header/footer text.
  Valid `PlcfSed` SED records also preserve each section's SEPX page size,
  orientation, nonnegative left/right/top/bottom margins, equal-width
  `sprmSCcolumns` counts from 1 through 44, strict `sprmSFTitlePage` first-page
  state, and source-order page-number format/restart state through
  `SectionSetup` and the final `DocSetup`, including headerless and
  single-section documents. Boundary SEPX records preserve new/even/odd
  `sprmSBkc` break kinds, title-page state, and bounded `sprmSNfcPgn` plus
  `sprmSFPgnRestart`/`sprmSPgnStart97`/`sprmSPgnStart` values through the shared
  model and fresh `.docx` conversion/reopen. Supported MSONFC values map to the
  existing model formats; valid unrepresentable values use a bounded decimal
  fallback, non-counting values use the spec-permitted decimal fallback, and
  invalid values leave prior state intact. A
  disabled restart ignores its stored start, while an enabled zero/default
  start normalizes to the model's one-based contract. An explicit unequal-
  spacing selector leaves the column count unmodeled; a later valid equal-
  spacing selector restores the last valid count. Malformed local SEPX data
  keeps that section's deterministic default without discarding valid
  neighboring sections.
  Continuous/new-column section marks normalize to the shared model's
  next-page fallback. Custom column widths/gaps, separator lines, manual column
  breaks, RTL column ordering, gutters/facing pages, header/footer distances,
  page borders/grids, vertical justification, negative fixed-position
  top/bottom semantics, display-number effects on physical pagination, and
  page-number footer inference remain outside this bounded reader path.
  Exact multi-note/endnote reference markers and exact text-box shape anchors
  are not yet fully promoted, so non-body regions still remain in the flat
  block stream;
  `Document::report()` emits `LegacyDocFlattenedSubdocuments` when FIB
  subdocument counts show that promotion is still incomplete.
- *`.docx` read only:* an original-view `DocModel` (accepted-current is the only
  modeled block view; original tracked-change text is exposed via
  `main_text_with_revision_view()` and `revisions()`, comments via `comments()`);
  accepted `main_text()`/`DocModel` content includes inline and block-level
  `w:ins`/`w:moveTo` current-content wrappers while omitting `w:del`/`w:moveFrom`
  old-content wrappers. Comment anchors plus `fields()`/`floating_shapes()` follow
  that same accepted-current policy, and `fields()` also uses the single-branch
  `mc:AlternateContent` policy so redundant Choice/Fallback field serializations
  do not duplicate side-table fields. Supported document-default, paragraph,
  and character style run properties are resolved through bounded `basedOn`
  chains, including the declared default paragraph style for unstyled
  paragraphs, as are paragraph direction/alignment/pagination, physical/logical
  side indents, tabs, spacing, first/hanging indents, flat RGB shading, and
  page-break-before. Table/list/conditional paragraph style effects and broader
  theme/automatic/pattern/nonzero line- or character-unit semantics remain
  unmodeled.
  Headers/footers, text boxes, footnotes/endnotes, and per-level numbering labels
  **are** now extracted; complete positive direct table grids whose column count
  matches the reconstructed cell/span grid populate normalized model column
  proportions, while invalid or count-mismatched grids retain the content-sized
  fallback; `header_footers()` exposes `.docx` referenced
  header/footer part records with `part#type` ids and default/even/first-page
  variants, while `DocSetup`/`SectionSetup` model default, first-page, and
  even-page variants for paragraph section breaks and the final section,
  including inherited defaults when a later section omits them;
  `notes()` exposes `.docx` footnote/endnote side-table records with
  Word ids, reference-id anchors, and normalized containing body block text for
  matched direct or accepted-current wrapped references; `text_boxes()` exposes
  `.docx` accepted-current body/note/header/footer text-box side-table records from
  `w:txbxContent`, and unambiguous anchored text boxes include containing body
  anchor text;
  `text()` includes headers/footers, `main_text()` is
  body-only; `core_properties()` exposes supported `docProps/core.xml` metadata
  fields including descriptive, package, timestamp, revision, and version values,
  while `report().custom_properties` exposes parsed string custom document
  properties.
- *Write/edit:* editing an opened `.docx` preserves arbitrary OOXML parts
  verbatim and the writer/edit surfaces are broad (see **Write** and **Edit**
  above). Structural edits now enumerate conservative atomic direct body
  paragraph/table/content-control subtrees, insert one plain direct paragraph,
  and move or remove exact retained subtrees. Nested-container edits, arbitrary
  rich block insertion/duplication, cross-block range rewriting, and
  relationship/media garbage collection are not implemented yet, along with
  newer extension chart families beyond the current authored set.
- *Render:* preview-grade vs LibreOffice (see above); Word-exact end-to-end RTL
  typography beyond bounded paragraph/run/list/table behavior; the core crate
  embeds no CJK font, so use a system font, `render_pdf_with_fonts`, or the
  optional `bundled-fonts` companion subsets.
- *Extracting text back out of rendered RTL PDFs:* complex scripts shape several
  glyphs from one cluster, which PDF can only map to text through the
  `ActualText` marked-content feature. rwml emits those spans, but support for
  them is not universal — Acrobat and Chrome honor them, while MuPDF/PyMuPDF,
  pdfminer.six, and pypdf do not and split Arabic words apart when copying or
  extracting. Rendering itself is unaffected, and Hebrew (one glyph per cluster)
  extracts intact. `scripts/render_validate.py` reads `ActualText` itself, so its
  own measurement is not affected. The public corpus's right-to-left list fixture
  still scores below the per-document floor for a narrower reason: its list
  marker is a separate text object, so an extractor's bidirectional pass leaves
  the marker's period beside the number rather than beside the following
  right-to-left word. Both renderers draw that period in the same place.

## Roadmap

The long-term native Word engine roadmap is summarized below.

Current maturity work is concentrated in deeper compatibility rather than new
top-level APIs. The bounded R2 reader/field pass and deterministic secondary-text
context work are closed; unresolved values remain cached with explicit reasons
where layout or Word behavior is required. The larger remaining projects are
Word-exact pagination and RTL typography, broader floating-object reflow,
nested/package-aware structural editing, and full vector metafile replay. Future
slices should move only with focused parser, renderer, report, or public-corpus
evidence.

Two conventions apply throughout this document. Everything described above the
roadmap is implemented and covered by tests — support is never claimed ahead of
a test, and each bounded slice names the cases it does not handle. Those named
limits describe the current build; they are not scope that has been declined.
The unchecked entries below are correspondingly open projects, not closed ones:
they stay unchecked until evidence closes them, and describing a gap as a
current limit never counts as finishing it.

- [x] Codepage-aware `.doc` text; encryption / Word 6/95 detection gates
- [x] Full read model: runs (CHPX incl. font/size/color and CHPX-resident
      highlighting plus direct super/subscript and literal caps/small-caps,
      plus bounded piece `Pcd.Prm` character formatting applied after CHPX:
      six literal `Prm0` toggles and precompiled literal `Prm1` toggles,
      underline, RTL, highlighting, and vertical alignment),
      paragraphs (bounded direct and style-derived flat-color
      `sprmPShd80`/`sprmPShd` shading),
      headings (STSH), tables (`sprmTDefTable` merges and bounded relative
      column proportions), list autonumbers, hyperlinks, inline images
- [x] Unified `.docx` reader into the same model (98.6% recall vs python-docx)
- [x] **`.docx` writer** - styled authoring (named styles, rich tables with typed nested cell blocks, page setup,
      styled runs, leveled lists, paragraph page-break-before, simple fields, `PAGEREF` helper runs, dirty TOC heading-range fields,
      run-anchored comments with reply parent ids and commentsExtended metadata, tracked insertion/deletion runs,
      run-level content controls with data-binding metadata, bookmarked runs, authored footnotes/endnotes, inline/standalone hyperlinks,
      string custom document properties, raw custom XML data-store items, explicit Word document ids, web-extension task pane package shells, styled default/first/even headers/footers + page numbers, section columns, document grids, text direction, title pages, page-number restarts/formats, next/even/odd section breaks, images with inline rotation and page-relative floating offsets,
      table width, fixed-layout tables, table alignment, indentation, authored column proportions, uniform/per-side border widths, styles, and colors, per-cell table margins,
      and the [core OOXML chart families](#chart-families) with embedded workbook-backed data) via `DocBuilder`,
      `ParagraphBuilder`, `RunBuilder`, `CommentBuilder`, `RevisionBuilder`,
      `ContentControlBuilder`, `TableBuilder`, `CellBuilder`, `ImageBuilder`,
      `ChartBuilder`, `DocModel`, and
      `write_docx`
- [x] **PDF renderer** - `parley` + `krilla` with rich text/tables/images,
      body and ordinary/recursively flattened table-cell list markers,
      hyperlinks, model-backed clockwise image rotation, six-way solid
      model-backed table border color/width, paragraph
      page-break-before, header-row repeat, oversized-row split,
      bounded DOCX document-default, declared-default-style, and explicit
      paragraph-style spacing, line height, first/hanging indents, flat
      shading, and page-break-before,
      direct DOCX table-cell margin defaults with row-local `w:tblPrEx`
      exceptions and per-side direct cell exceptions,
      direct plus bounded whole/first/last/horizontal-band table-style DOCX
      table-row `cantSplit`, including recursively flattened nested rows,
      direct DOCX and recursively nested table-cell keep/widow controls, direct and
      paragraph-style-inherited legacy DOC
      `sprmPFKeep`/`sprmPFKeepFollow`/`sprmPFWidowControl`/`sprmPFPageBreakBefore`
      plus bounded direct/style top-level
      `sprmPDyaBefore`/`sprmPDyaAfter`/proportional `sprmPDyaLine` spacing
      (explicit before/after spacing also reaches table cells) and direct row
      `sprmTFCantSplit`/`sprmTFCantSplit90`, font registration
- [x] Reader: `.docx` headers/footers, text boxes (`w:txbxContent` incl. run-level
      `mc:AlternateContent`) including `text_boxes()` records, footnotes/endnotes
      including `notes()` records, per-level numbering labels, caps
- [x] Renderer: model-driven page geometry (size/orientation/per-side margins);
      running headers/footers; nested-table-cell text; common Symbol/Wingdings
      display mapping
- [x] Reader: `.docx` comments with body/note/header/footer anchors,
      body/note/header/footer tracked-change views and side-table extraction,
      core document metadata, body/note/header/footer field detection,
      body/note/header/footer floating-shape geometry and
      containing-block anchor text capture, trusted body `PAGE` computation
      plus `FILENAME`/`MERGEFIELD`
      render support, document-info/date/stat
      cached-display support, deterministic literal arithmetic formula fields,
      literal `QUOTE`, literal `IF`, literal `COMPARE`, explicit-default
      `FILLIN`/`ASK`, and literal `SET`
      bookmark assignments feeding later plain `REF`/direct bookmark references
      plus source-order bookmark-backed `IF`/`COMPARE`/`NEXTIF`/`SKIPIF`
      comparisons and ordinary document-bookmark-backed `IF`/`COMPARE`/`NEXTIF`/`SKIPIF`
      comparisons,
      dynamic/control,
      inserted-content, and mail-merge helper field diagnostics, reference/index field diagnostics,
      numbering/list field diagnostics, document-structure field diagnostics,
      display/layout field diagnostics, action/automation field diagnostics,
      compatibility/private field diagnostics, barcode field diagnostics,
      legacy form field diagnostics plus deterministic checkbox checked/default
      states, dropdown result/default selections, explicit non-empty text-input
      current results, and empty-current text-input default results,
      unambiguous `.docx` `REF`
      bookmark text computation
      including Word-generated hidden bookmark targets and multi-paragraph
      bookmark ranges plus inline tabs, line breaks, and no-break/soft
      hyphens for simple and common complex body fields plus deterministic
      `REF \* Upper`/`REF \* Lower`/`REF \* Caps`/`REF \* FirstCap` text
      format switches, source-order `REF \p`
      relative-position results, explicit numbered-paragraph `REF \n` labels
      from single-branch source paragraphs including `\n \p` relative suffixes
      and `\n \t` numeric-text suppression, `REF \r` relative-context labels
      including `\r \p` relative suffixes and `\r \t` numeric-text
      suppression, `REF \w` full-context labels including `\w \p` relative
      suffixes and `\w \t` numeric-text suppression, `REF \f` note-reference
      marks for bookmarks around body footnote/endnote references with
      generated REF note marks counted in source order plus common field-result
      number/text format switches, text-neutral `REF \d "separator"` bookmark
      text while value-changing sequence/page separator cases preserve cached text,
      direct bookmark-name field computation with
      supported text-format switches, neutral `\h`, explicit-number `\n`, `\n \t`, `\r`, `\r \t`, `\w`, `\w \t`, note-reference `\f`, sequence-separator `\d`, and source-order `\p`,
      bookmarked `NOTEREF`/legacy `FTNREF` footnote/endnote reference marks with
      neutral `\h`, note-reference-style `\f`, source-order `\p` above/below
      results, and common field-result number/text format switches, bare default `TOC`,
      standalone bookmark-scoped default `TOC \b`,
      plus explicit `TOC \o` heading-outline computation, including omitted all-level ranges and common
      `\o`/`\u` combinations, with neutral `\h`/`\z` switches,
      text-preserving `\w`/`\x` switches normalized to plain text, text-neutral
      `\n` no-page-number, `\p` entry/page separator, and `\d`
      sequence/page separator switches, `\s` sequence-number page prefixes,
      deterministic TOC `\* Upper`/`\* Lower`/
      `\* Caps`/`\* FirstCap` field-result format switches, neutral TOC
      `\* MERGEFORMAT`/`\* MERGEFORMATINET`/`\* CHARFORMAT`, plus
      quoted or switch-delimited unquoted `TOC \t` custom-style entries, `TOC \f` entries from matching
      `TC "Text"` markers with optional `\f` type identifiers, `\l` levels,
      and common marker text-format tails,
      `TOC \c` full-caption entries and `TOC \a` label/number-omitted
      caption-text entries from paragraphs containing matching
      `SEQ Identifier` fields, with simple or common complex dirty/stale `SEQ`
      caption numbers recomputed from source order,
      standalone `TOC \u` explicit paragraph
      outline-level computation and `TOC \b` bookmark-scoped computation when
      the bookmark range is recoverable, including empty computed results for
      existing scopes with no matching entries, with normalized simple inline
      heading/caption tabs, line breaks, no-break/soft hyphens, and supported
      literal symbols for simple and common complex
      fields, body `PAGE` trusted current-page computation with page-number and
      field-result format switches, named `PAGEREF` classification with leading
      hard-break,
      paragraph page-break-before, structural section-start, default next-page
      section-start, deterministic section page-number restart labels,
      supported section page-number format styles, source rendered page-break, and trusted
      rendered-context hard-break target computation,
      deterministic page-number and field-result format switches, trusted
      leading-structural, source-marker, hard-break-after-target, and
      paragraph-end section-break `\p` relative-position computation, plus
      cached page-reference result preservation for remaining
      layout-dependent cases, cached field result preservation for inline tabs, line
      breaks, and no-break/soft hyphens in simple and common complex body
      fields, `.docx` running header/footer
      default selection/inheritance, first/even-page variant modeling and
      authoring, plus section-aware first/even-page render selection, and
      Symbol/Wingdings glyph mapping
- [x] Reader R2-a: field report/evaluator parity for value-changing fields
      where duplicated syntax checks or document-report/render-report
      diagnostics can drift from computed-result behavior. Verified parity
      coverage now locks `PAGEREF`, `REF`, `NOTEREF`/`FTNREF`, and TOC
      computed/gap buckets across opened-document and render-model reports, and
      empty unsupported simple/complex field instructions plus supported hidden
      `RD`/`TA`/`XE` marker fields stay reportable in model/render inventories;
      reopen only for newly proven parser/evaluator/report drift or exact
      duplicated syntax logic.
- [x] Reader R2-b: bounded deterministic `PAGE`/`PAGEREF` computation in trusted
      leading/source-rendered, section-start, paragraph-end section-break
      target, source-marker, and hard-break contexts. Remaining Word-exact
      current-page, page-reference, and relative-position cases are an inherent
      layout ceiling and stay cached; opt-in `layout_pages_with_fonts` reports
      rwml's own preview-grade pagination without changing reader results
- [x] Reader R2-c: deterministic value-changing `REF` (incl. `\#` numeric
      picture and `\!`), `NOTEREF`/`FTNREF` (incl. `settings.xml`
      `numStart`/`numFmt` and `customMarkFollows`), and TOC heading-source
      `NOTEREF`/`SEQ` resolution; the remaining REF/NOTEREF/TOC cases are
      layout- or Word-behavior-dependent and stay cached-with-reason
- [x] Reader R2-d: non-deterministic data-, source-, layout-, action-,
      generated-, barcode-, compatibility-, and protected-form field families
      preserve cached text and stay reportable by design unless deterministic
      semantics are proven
- [x] Reader R2-e: bounded legacy `.doc` note anchors from
      `PlcffndRef`/`PlcfendRef`, count-aligned text-box anchors from `PlcSpaMom`,
      annotation author metadata, and per-section `PlcfHdd` header/footer story
      application through `PlcfSed`, plus SED/SEPX-backed page size,
      orientation, nonnegative per-side margins, title-page state, equal-width
      columns, and page-number restart/format state for single, headerless, and
      multi-section documents. Missing annotation bookmark tables,
      count-mismatched shape/text-box tables, malformed local SEPX data, and
      unsupported section properties retain bounded fallbacks by design
- [x] Reader side-table context parity: supported `STYLEREF` fields compute in
      accepted-current insertion/move-to revision text and note-reference anchor
      text with source-order-stable body context. Deletion/move-from text keeps
      cached original results, note anchors skip old-content fields, and
      revision-view context reconstruction matches strict open-time part parsing,
      source package size, and `settings.xml` note numbering
- [x] **Package-preserving edit layer** — `Document::open`→edit→`save` keeps every
      unmodeled part verbatim; the element-tree edit methods (text/field/comment/
      note/image/content-control/revision/core-property plus conservative atomic
      direct body block enumeration/plain-paragraph insertion/move/removal,
      listed under
      [Edit](#edit--open-change-save-package-preserving)) preserve fields/shapes/
      content-controls/comments/revisions;
      `edited_parts` exposes touched package parts; `edit_capability` /
      `report().edit` expose read-only reasons; `opc` + `xmltree` internals;
      fallible `try_write_docx`
- [ ] Renderer: Word-exact pagination beyond bounded section columns, opened-DOCX
      top-level/direct-and-nested-cell keep/widow controls, and direct or
      paragraph-style-inherited legacy-DOC top-level/ordinary-cell paragraph
      plus direct row controls; floating-shape wrap/reflow
      beyond bounded forward page-wide `wrapTopAndBottom`,
      full layout-derived `PAGE`/`PAGEREF` values beyond trusted source markers,
      remaining render-time TOC/REF/NOTEREF policy where layout context is
      required, broader bundled script coverage, and full Word-exact RTL typography
- [x] Authoring API, native PDF preview rendering, and embedded workbook-backed
      data for the [core OOXML chart families](#chart-families)
- [x] Wireframe styling for authored surface and 3-D surface charts
- [x] Shape styling for authored 3-D bar and 3-D column-family charts
- [x] Metafile diagnostics for WMF/EMF/EMZ/WMZ path, format, byte size, compression flag, and raw/gzip-wrapped header dimensions
- [x] Chart-ex extension chart families (waterfall, treemap, sunburst, histogram, box & whisker, funnel) authored as `chartEx` parts
- [x] Bounded exact single-DIB metafile (WMF/EMF) raster extraction for palette, RGB, and strict bitfield rasters rendered as images
- [ ] Full vector metafile (WMF/EMF) rendering beyond single-DIB raster extraction and bounded header diagnostics

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The full local gate is
`python3 scripts/public_hygiene_audit.py`, `cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings`,
`cargo clippy --all-targets --all-features -- -D warnings`,
`cargo test --all-targets`, `cargo test --no-default-features`,
`cargo test --all-targets --features render`, `cargo test --doc --all-features`,
and `cargo doc --no-deps --all-features`.

## License

Licensed under the [MIT License](LICENSE). Third-party dependency licenses are
listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md). This crate implements
only the publicly documented [MS-DOC] / [MS-CFB] / OOXML specifications and contains
no Microsoft source.

## Trademarks

`rwml` takes its name from **WordprocessingML**, the ECMA-376 markup for
word-processing documents. It is an independent open-source project, **not**
affiliated with, authorized by, or endorsed by Microsoft. Microsoft, Microsoft
Word, and the `.doc` / `.docx` file formats are trademarks or registered
trademarks of Microsoft Corporation, referenced here only descriptively to
indicate file-format compatibility. The crate is built solely from the publicly
documented [MS-DOC] / [MS-CFB] / OOXML specifications and contains no Microsoft
source code.

[MS-DOC]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/
[MS-CFB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/
[UAX #14]: https://unicode.org/reports/tr14/
[`Document::open`]: https://docs.rs/rwml/latest/rwml/struct.Document.html#method.open
[`DocModel`]: https://docs.rs/rwml/latest/rwml/struct.DocModel.html
[`DocBuilder`]: https://docs.rs/rwml/latest/rwml/struct.DocBuilder.html
[`Error`]: https://docs.rs/rwml/latest/rwml/enum.Error.html
