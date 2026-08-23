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
let imgs  = doc.images();        // raster bytes plus per-drawing DOCX alternative text
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
offsets (including per-occurrence `wp:docPr/@descr` alt text from opened DOCX
drawings through Markdown/HTML and fresh DOCX conversion), simple fields with
cached results, `PAGEREF` helper runs, dirty TOC
heading-range fields, run-anchored comments with reply parent ids and
commentsExtended metadata, tracked
insertion/deletion runs, run-level content controls, bookmarked runs, authored
footnotes/endnotes, string custom document properties, raw custom XML data-store
items, generated core metadata (title, subject, creator, description, keywords,
category, content status, last modified by, created, modified, last printed,
revision, and version), explicit Word document ids, web-extension task pane package shells, page setup with section columns, document grids, text direction, title pages, and page-number restarts/formats, explicit page breaks and next/even/odd section breaks,
styled default/first/even running headers/footers (including nested rich tables
with local hyperlinks, real raster images and charts, and visible missing-image
fallbacks), and page numbers all round-trip. Content controls can include tag/alias and
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
bar/column-family charts. Fresh rwml-generated charts with serialized series are
reconstructed as modeled `Chart` blocks on native reopen, including titles,
series data, dimensions, alternative text, wireframe state, and 3-D shape state,
so reopened documents use the same vector PDF painter. Core scatter/bubble
payloads serialize generated numeric x-values rather than the public string
category labels, so `categories` normalizes to empty for those families on
reopen. Formula/reference-backed, combination, and otherwise arbitrary Office
chart payloads remain package-preserved and explicitly reported as unsupported.
Generated charts use part-local relationships in default/first/even headers and
footers, including ordinary and nested running-table cells, and survive native
reopen into the same model and PDF path. A running chart without a serialized
series retains an escaped visible fallback and emits no orphan chart package
parts.
See [`examples/report.rs`](examples/report.rs).

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

`set_table_cell_text` replaces direct text in a selected top-level table cell,
including a parent cell that contains a nested table; the nested table's text
and XML structure are excluded from the replacement and preserved. Nested-grid
layout remains a renderer ceiling.

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
subtrees and untouched package parts are preserved. Removing a block also prunes
an unreferenced internal image relationship and its unreachable `word/media/*`
target when the retained relationship graph proves it is safe; other relationship
kinds, shared media, and general package garbage collection remain outside this
bounded behavior. Read/model views remain stale until explicitly refreshed or
reopened.
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
whole-degree clockwise rotation, decoded block or reader-style inline raster
images plus non-empty model-authored charts inside ordinary modeled table cells,
and decoded raster images, model-authored charts, and ordinary modeled tables in
selected default/first/even running headers and footers are drawn. PDF hyperlinks
emit **clickable annotations**.
Running paragraph and table-cell hyperlink annotations follow the selected
surface variant and are clipped to the physical page plus the visible
margin-band line or row fragment, so clipped table content cannot leave an
active link over the body.
Running-surface images retain
their rotation-aware bounds, are proportionally reduced without upscaling to
the remaining section-local margin band, and are centered within its content
width. Non-empty running-surface model charts use the body vector painter and
are proportionally reduced without upscaling to the remaining band, centered,
and clipped at their fitted bounds; labels, strokes, and geometry scale together.
Arbitrary opened Office chart-part modeling remains outside this path.
Running-surface tables use the body
row painter for model-backed width,
alignment, visual RTL placement, borders, cell shading, margins, and vertical
alignment. Fitting rows paint in source order; a first row taller than the
remaining band paints once under a vertical clip and ends that surface without
moving body content or changing page counts. Opened-DOCX running-table cells
consume the same resolved explicit LTR/RTL tab-stop, leader, and bar-tab subset
as body table cells, together with a finite positive settings-defined default
interval. Decoded block and reader-style inline rasters plus non-empty
model-authored charts paint in cell source order, center within the inner cell,
fit proportionally without upscaling to the inner width and active page content
height, contribute to row measurement and vertical alignment, and stay atomic
across row splits and repeated headers. Source-only pagination hints inside those
cells, nested-grid geometry, inline media baseline/crop/flip/effects, arbitrary
opened Office chart parts, and Word-exact overlap remain outside this bounded
path.
Finite positive
paragraph before/after gaps advance later running text, pictures,
model-authored charts, and tables in source order; explicit zero suppresses a
gap, and an unset after value retains the renderer's established default
trailing gap. Gaps are bounded to the remaining margin band, footer page numbers
follow the resulting cursor, and body page counts do not change. Adjacent gaps
are additive rather than Word-exact collapsed. Opened `.docx` sections apply
explicit unsigned `w:pgMar` header and footer distances, and opened legacy
`.doc` sections apply validated unsigned `sprmSDyaHdrTop` and
`sprmSDyaHdrBottom` values, through a private section-aligned render sidecar.
Headers start at the requested offset from the physical top edge; footer
content is bottom-aligned to the requested offset from the physical bottom
edge; generated DOCX page numbers follow that same path. Both surfaces clamp at
the body margin rather than overlap body content, and missing or malformed
values retain the existing fixed preview bands. Public authoring of distances,
reconstruction of legacy installation-language defaults, automatic margin-
conflict resolution, and Word-exact overlap remain outside this bounded
source-only path. Fresh `Document::to_docx()` conversion of an opened DOCX or
legacy `.doc` carries aligned, finite source distances from zero through 31,680
twips into each generated section; absent, out-of-range, or misaligned hints
fall back locally to the writer's 708-twip default, and native reopen recovers
the emitted values. Standalone model writing retains that default because the
public model has no distance fields. Opened `.docx`
tables
materialize direct `dxa`/`nil` `w:tblCellMar`
defaults and per-side direct `w:tcMar` exceptions, including logical
`start`/`end` mapping under `w:bidiVisual`, and inherit a table style's own
`w:tblCellMar` through its `w:basedOn` chain, plus its `wholeTable`
and bounded horizontal/vertical bands, first/last rows and columns, and four
corner conditional regions beneath those direct declarations. Selection honors
named or hexadecimal `w:tblLook`, explicit row/cell `w:cnfStyle`, Word's 0-3
row/column band sizes, repeated header rows, merged-cell ownership, and RTL
corner mapping. A row-local
`w:tblPrEx/w:tblCellMar` replaces the direct table margin property for that row,
inherits omitted sides from the style or schema defaults, and remains beneath a
direct cell declaration. Vertical-band, column, and corner conditional regions
apply only to the model-backed cell presentation subset; conditional borders and
paragraph/run formatting remain outside it. A table style's `w:tblBorders`, from
its own `w:tblPr` or its `wholeTable` region and resolved through the same chain,
applies when the table declares no borders of its own. A style's `w:tblW`,
`w:tblInd`, and table `w:jc` fill in only what the table itself leaves unset. Its bounded
conditional `w:tcPr` regions supply cell margins, shading, vertical alignment,
and percentage preferred width for cells that declare none; preview grid
geometry still comes from table/grid widths rather than per-cell preferred
width. A style's
`w:tblLayout` and `w:bidiVisual` apply
when the table declares neither.
Rotated raster bounds drive proportional
content-box/page-height fitting and pagination. Narrow RTL tables reverse logical
placement and mirror their cells inside the local table box. Page geometry,
equal-width section columns, bounded explicit unequal-width opened-DOCX and
legacy `.doc` tracks, explicit opened-DOCX and legacy `.doc` inter-column
spacing and column-separator rules, source-opened `w:sectPr/w:bidi` and
`sprmSFBiDi` right-to-left column population, visible top-level
opened-DOCX and legacy `.doc` manual column breaks, and per-side margins
come from the document;
multi-page tables repeat their header rows without losing outer placement or
border paint. Opened
`.docx` rows, including recursively flattened nested rows, may split across
pages by default, while effective `w:cantSplit` from direct row properties or
an inherited table-style chain keeps a fitting row together and an over-tall
row still splits at line boundaries. Table styles include direct non-conditional
row properties and bounded `wholeTable`, horizontal and vertical bands,
first/last rows and columns, and all four corner conditional row properties
selected by direct table `w:tblLook` or row `w:cnfStyle`. Named and hexadecimal
selector masks, inherited style and direct-table `w:tblStyleRowBandSize` /
`w:tblStyleColBandSize` values use Word's 0-3 grouping rules; the effective
table visual direction maps physical corners, later regions and direct row
formatting retain Word precedence. Model-only renders
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
left/start-aligned top-level body paragraphs, plus explicit left-aligned stops
and default-tab fallback in supported center/right/justified-aligned LTR
top-level body paragraphs, and default, logical-start, center/end, and decimal
tab stops in RTL
right/start-aligned top-level body paragraphs.
LTR stops retain their page-text-margin coordinates under supported left,
positive first-line, and hanging indents; RTL logical-start stops retain
leading-edge coordinates from the right page-text margin under resolved
physical or logical left/right indents. Default-tab fallback targets use the
same margin-anchored grid, use a positive `w:defaultTabStop` interval from
`word/settings.xml` when present, and are clamped to the active paragraph box.
Opened documents also honor document-default,
declared default paragraph-style, and bounded explicit paragraph-style
before/after spacing, proportional automatic line spacing, first-line/hanging
indents, flat RGB shading, and `pageBreakBefore`; authored zero/off direct
overrides; and source-aligned `keepNext`,
`keepLines`, and default-on `widowControl` pagination hints in top-level body
paragraphs and direct or accepted-current wrapper-contained paragraphs in
ordinary or recursively nested table cells, without adding those source-only
layout hints to the public `DocModel`. Fresh conversion of an opened legacy or
DOCX document serializes the effective keep controls and widow-off state for
aligned top-level body paragraphs and aligned direct paragraph blocks in
surviving cells of top-level tables, plus effective no-split state for aligned
top-level table rows. The same direct body subset carries resolved exact/minimum
line rules and explicit tab stops into fresh DOCX conversion. Visible manual
column breaks in aligned top-level body paragraphs also survive through a
strictly validated source-offset bridge. Direct top-level paragraphs in selected
default/first/even running headers and footers from an opened DOCX also retain
reader-resolved explicit tab stops through a section-aligned six-variant bridge.
Direct paragraph blocks in surviving cells of top-level tables on those same
running surfaces retain explicit tabs through a companion block/row/surviving-
cell/paragraph-aligned bridge. Direct top-level running paragraphs also retain
reader-resolved exact/minimum line rules through a section-aligned source-only
bridge. Nested-table descendants and notes remain outside these fresh-conversion
paths; running-table-cell paragraphs remain outside line-rule conversion and all
running surfaces remain outside pagination conversion, while legacy-DOC running
stories and nested running-table descendants remain outside tab conversion.
Settings-defined default-tab intervals remain outside the tab path, and table-
cell, note, running-surface, and nested-content manual breaks remain outside the
column-break path.
Ordinary top-level paragraphs in selected default/first/even running headers
and footers from an opened DOCX also consume reader-resolved explicit tab stops
and supported leaders through section-aligned render hints. Default-surface
inheritance, page-variant selection, and a positive settings-defined default-tab
interval use the same bounded paragraph shaper without changing the public
model; running-table-cell tabs remain independently aligned. Legacy-DOC running
stories, post-tab field containment, and Word-exact tab reflow and pagination
remain outside this path.
Ordinary top-level real footnote and endnote paragraphs from an opened DOCX
likewise consume reader-resolved explicit tab stops, supported leaders, and a
positive settings-defined default-tab interval. Their private hints retain
footnote-then-endnote block order across skipped separator records and preceding
non-paragraph blocks without changing the public model. Direct paragraphs in
cells of top-level real footnote and endnote tables use the same bounded tab
path through a block/row/surviving-cell/paragraph-aligned private tree, including
vertical-merge owner indexing. Nested note tables, legacy-note tabs, fresh note
conversion, post-tab field containment, page-bottom note composition, and
Word-exact tab reflow and pagination remain outside this path.
Resolved LTR tab stops in ordinary and
recursively nested table-cell paragraphs use the same bounded path as supported
top-level paragraphs. Explicit left-aligned LTR stops in center-, right-, and
justified-aligned paragraphs use that path when the resolved stop is reachable.
Top-level opened-DOCX body paragraphs, ordinary or recursively nested
table-cell paragraphs, real footnote/endnote paragraphs, and ordinary
paragraphs in default/first/even running headers and footers, including their
modeled table cells, also consume resolved `exact` and `atLeast` `w:lineRule`
values as twentieths of a point through source-only render hints. Exact boxes
center fitting text, bottom-align and vertically clip over-tall text, while
at-least boxes only expand shorter natural lines. The hints remain aligned when
a source paragraph is split by an explicit page break, with surviving
vertical-merge owner cells, across skipped note separator records, and through
section-specific default-surface inheritance, page-variant selection, and
non-table block positions before a running table. Exact running-surface content
is clipped to its margin-band line or visible row fragment. Notes retain the
preview renderer's flattened end-of-flow placement; page-bottom note
composition, model-authored exact/minimum paragraph rules, and note,
nested-table-descendant, or running-table-cell fresh conversion remain outside
this absolute-spacing path. Direct top-level opened-DOCX running paragraphs use
the bounded fresh-conversion bridge described above.
The same bounded path reaches ordinary RTL table-cell paragraphs for
center/end/decimal stops. Supported LTR and RTL dot, hyphen, underscore,
heavy, and middle-dot leaders plus bar tabs now paint through the same bounded
sidecar path in top-level and table-cell paragraphs. Settings-defined
default-tab intervals in unsupported contexts and implicit hanging-indent/
list-marker tabs remain outside this
bounded tab support. Within
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
mirroring. Source-opened DOCX section `w:bidi` and legacy `sprmSFBiDi` also
populate equal or bounded unequal section columns from right to left without
forcing paragraph direction. This improves mixed Arabic/Hebrew documents
without claiming Word-exact list-level alignment, punctuation, or table
typography.
`ListInfo` does not retain list-instance identity, source `numId`/`ilfo`,
restart/start overrides, marker fonts or glyph metadata, or marker
tabs/alignment/exact hanging indents. Independent or restarted empty-label
lists therefore use deterministic preview numbering rather than Word-exact
numbering. Marker-aware table autofit, nested-grid geometry, Word-exact inline
media placement/effects, legacy nested-table recovery, and Word-exact RTL list
typography remain outside this preview.
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
Opened legacy documents additionally retain positive non-multiple LSPD as a
minimum line box and negative encoded LSPD as an exact line box through private
hints. Aligned top-level paragraphs and direct paragraph blocks in surviving
cells of aligned top-level tables carry those rules into fresh `.docx`
conversion/reopen, while PDF previews also apply them to ordinary main-story
table-cell paragraphs and ordinary top-level or table-cell paragraphs in
section-linked even/default/first headers and footers. Exact boxes use the same
centered or bottom-aligned baseline and vertical clipping behavior as opened
DOCX. The hints survive promoted manual page-break fragments, remain aligned
through horizontal cell folds and surviving vertical-merge owners, and mirror
the existing six-story legacy running-surface mapping, non-table block
positions, and unindexed default-header fallback. Exact running-surface content
is clipped to its margin-band line box or visible table-row fragment.
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
At-least/exact and explicit zero proportional LSPD values clear an inherited
multiplier but remain unset in the shared model because it has no corresponding
line-rule representation. The absolute line-rule sidecar does not enter the
shared model; nested-table-descendant, note, and running-surface rules are not
yet carried through fresh `.docx` conversion.
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
> layout, end-to-end RTL typography, page-bottom footnote composition, exact
> per-column rewrapping, or Word-exact section-local geometry. Supported section
> breaks apply each section's modeled physical page width and height, including
> landscape layouts, plus per-side margins to native shaping, pagination,
> running surfaces, anchored overlays, and emitted PDF pages. Explicit-false
> opened-DOCX unequal sections accept one through 64 bounded direct `w:col`
> widths and following spaces, preserve fitting geometry, and scale an over-wide
> set only while every scaled column remains usable; otherwise they fall back to
> equal tracks. Complete unequal legacy `.doc` sections accept two through 44
> indexed `sprmSDxaColWidth` values and optional zero-defaulted
> `sprmSDxaColSpacing` values under a false `sprmSFEvenlySpaced` selector.
> Content is shaped conservatively to the narrowest active track before
> pagination places it at each declared origin. Equal-width opened DOCX and
> legacy `.doc` sections apply explicit `w:cols/@w:space` and
> `sprmSDxaColumns` values. Valid opened-DOCX `w:cols/@w:sep` and legacy
> `sprmSLBetween` flags paint centered rules between every active equal,
> fitting, scaled, or fallback track without changing pagination; one-column
> sections remain paint-inert. Valid opened-DOCX section `w:bidi` and legacy
> `sprmSFBiDi` values preserve physical track geometry while starting flow at
> the rightmost track, advancing left, and resetting right on a new page.
> `Document::to_docx()` maps those validated opened-DOCX and legacy equal gaps,
> complete unequal tracks, separator flags, and section direction into fresh
> `w:cols`/`w:col` and `w:bidi` properties. Standalone `write_docx` remains
> model-only. Incomplete custom legacy geometry, public model authoring of
> private geometry, exact per-column rewrapping, and Word-exact reflow remain
> outside this bounded bridge.
> Unknown fields, remaining
> layout-dependent TOC/REF/NOTEREF cases, and unsupported value-changing field
> semantics retain their cached display text with diagnostics.
> `w:tblPrEx` remains a table-property exception path for the supported
> row-local `w:tblCellMar` behavior; other `w:tblPrEx` properties are outside
> this renderer slice, and `w:cantSplit` remains a `w:trPr` property.
> Exact/at-least line rules outside the bounded opened-DOCX top-level body,
> recursively flattened body-table-cell, real-note, ordinary running-surface,
> and running-table-cell paragraph paths,
> nonzero line-unit before/after spacing, enabled automatic/contextual paragraph
> spacing, nonzero character-unit indents,
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
> decoded raster around its center. Running-surface pictures use decoded/model
> raster dimensions rather than relationship display extents. Source-authored
> table-cell rasters and model-authored charts use atomic centered records fitted
> to the inner cell and active page content box. Source-authored display extents,
> crop/flip/effects, floating-anchor offsets, exclusion-zone reflow, arbitrary
> opened Office chart parts, and Word-exact inline baseline or header/footer
> overlap remain outside this bounded image bridge.
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
running surfaces and section-relative odd/even header selection remain outside
this bounded behavior; modeled section-local page geometry is applied, while
bounded explicit unequal DOCX and legacy `.doc` tracks use the same
deterministic private geometry as PDF output. Exact per-column rewrapping and
Word pagination remain outside it.

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
per-side margins, title-page state, all six section text directions,
document-grid state, and page-number restart/format state,
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
Converting an opened `.doc` or `.docx` through `Document::to_docx()` additionally
retains validated source-only section column gaps, complete unequal geometry,
separator flags, and right-to-left population without exposing them in the
public model. Legacy- and DOCX-backed conversion also retain validated
exact/minimum line rules plus effective `keepNext`, `keepLines`, and widow-off
state for aligned top-level body paragraphs and aligned direct paragraph blocks
in surviving cells of top-level tables, plus effective no-split state for
aligned top-level table rows. The same direct body subset retains resolved
explicit paragraph tab stops, while aligned top-level body paragraphs retain
visible manual column breaks through validated source character offsets.
Standalone model writing remains proportional-only and consumes no private
layout hints. Direct top-level paragraphs in selected default/first/even running
headers and footers from an opened DOCX also retain reader-resolved explicit tab
stops through section-aligned private hints. Direct paragraph blocks in
surviving cells of top-level tables on those running surfaces use a companion
block/row/surviving-cell/paragraph-aligned bridge. Direct top-level running
paragraphs also retain reader-resolved exact/minimum line rules through a
section-aligned source-only bridge. Nested-table descendants and notes remain
outside these fresh-conversion paths; running-table-cell paragraphs remain
outside line-rule conversion and all running surfaces remain outside pagination
conversion, while legacy-DOC running stories and nested running-table
descendants remain outside tab conversion. Settings-defined default-tab
intervals remain outside the tab path, and table-cell, note, running-surface,
and nested-content manual breaks remain outside the column-break path.

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
| **NOTEREF / FTNREF** | Footnote/endnote reference marks (honoring `settings.xml` `numStart`/`numFmt`, materializing a bookmarked literal mark immediately following `w:customMarkFollows`, and skipping its auto-number), `\h`, `\f`, `\p` above/below, number/text format switches | Missing targets → `UnresolvedBookmark`; no note mark or custom mark without a bounded following literal → `NoComputedResult`; per-page note restart is layout-dependent |
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
`.docx` corpus it reaches **1.000 mean text recall** with a **1.00 mean page-count
ratio** (extracting headers/footers,
text boxes, nested tables, real list labels, caps; model-driven page geometry makes
`.doc` page counts line up). All 21 public render fixtures score at least the
0.97 per-document floor on the strict revision-bound report, with stable
LibreOffice references. It still trails
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
The tag-bound release workflow generates the strict render and extraction
reports on the tagged revision, passes them with the hygiene and public-corpus
manifests to `release_manifest.py --enforce-policy-inputs`, and uploads the
exact crates plus evidence to both workflow artifacts and the GitHub Release.
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
  state, all six `sprmSTextFlow` directions, complete `sprmSClm` document grids
  with valid `sprmSDyaLinePitch` and representable nonnegative
  `sprmSDxtCharSpace`, and source-order page-number format/restart state through
  `SectionSetup` and the final `DocSetup`, including headerless and
  single-section documents. Boundary SEPX records preserve new/even/odd
  `sprmSBkc` break kinds, title-page, text-direction, and document-grid state,
  plus bounded `sprmSNfcPgn` and
  `sprmSFPgnRestart`/`sprmSPgnStart97`/`sprmSPgnStart` values through the shared
  model and fresh `.docx` conversion/reopen. Supported MSONFC values map to the
  existing model formats; valid unrepresentable values use a bounded decimal
  fallback, non-counting values use the spec-permitted decimal fallback, and
  invalid values leave prior state intact. A
  disabled restart ignores its stored start, while an enabled zero/default
  start normalizes to the model's one-based contract. A complete explicit
  unequal-spacing section recovers two through 44 indexed column widths and
  optional following spaces into a private preview sidecar while exposing the
  validated count through the shared section model. Widths are bounded to the
  specified 718 through 31,680 twips, spacing defaults to zero and is bounded to
  31,680 twips, and later valid indexed modifiers replace earlier values. A
  missing width leaves that unequal count unmodeled; a later valid equal-spacing
  selector restores the last valid count. Malformed local SEPX data keeps that
  section's deterministic default without discarding valid neighboring
  sections. Strict source-order `sprmSLBetween` Bool8 values reach a private
  section-aligned PDF sidecar; invalid later values preserve the last valid
  state, and one-column sections emit no rule.
  Strict source-order `sprmSFBiDi` Bool8 values similarly populate equal or
  complete unequal section columns from right to left in opened-document PDF
  previews; invalid later values preserve the last valid state and malformed
  local SEPX data remains isolated. This section direction does not force
  paragraph or run bidi behavior. Legacy-backed `Document::to_docx()` also
  serializes each aligned, validated equal gap or complete unequal layout,
  separator flag, section direction, and header/footer distance as bounded
  `w:cols`/`w:col`, `w:bidi`, and `w:pgMar` properties; native reopen recovers
  the same source semantics.
  Opened-DOCX conversion uses the same bounded writer path. Standalone model
  writing remains unchanged.
  A visible end-of-column character (`0x0E`) in a top-level main-story
  paragraph advances an opened-document PDF preview to the next active column,
  or to a new page after the final column, through private source-aligned
  offsets. The public model retains its newline representation. Hidden
  characters, table cells, non-main stories, and fresh conversion do not
  activate that preview hint.
  An internal main-story end-of-section character (`0x0C`) becomes the shared
  `PageBreak` block for native preview, export, and fresh `.docx` conversion.
  The final `0x0C` in each non-final `PlcfSed` range is consumed only as that
  section's terminator; repeated and marker-only manual page breaks remain
  explicit. Table-cell and non-main-story occurrences retain their newline
  representation.
  Continuous/new-column section marks normalize to the shared model's
  next-page fallback. Incomplete custom column geometry, gutters/facing pages,
  header/footer margin-growth semantics,
  page borders, vertical justification, signed negative document-grid
  character-pitch deltas, negative fixed-position top/bottom semantics,
  display-number effects on physical pagination, and page-number footer
  inference remain outside this bounded reader path.
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
  extracts intact. `scripts/render_validate.py` reads `ActualText` itself and
  conservatively normalizes a standalone period beside an RTL list label, so
  its strict public-corpus measurement does not treat that PDF text-object
  boundary as lost content.

## Roadmap

The public roadmap is deliberately capability-focused. Each supported behavior
above is covered by deterministic tests; the renderer remains a preview engine
rather than a Word- or LibreOffice-exact layout replacement.

| Area | Current direction | Explicit boundary |
|---|---|---|
| Read and fields | Keep extending bounded DOC and DOCX parsing, field evaluation, and cached-with-reason diagnostics | Layout-dependent field values remain cached when their page or Word context cannot be derived deterministically |
| PDF preview | Improve paragraph, table, tab, list, image, and section behavior from existing model data | Word-exact pagination, footnote composition, exact per-column rewrapping, and full floating-shape exclusion reflow remain out of scope |
| RTL | Extend tested mixed-script paragraph, table, tab, and list behavior | End-to-end RTL typography, punctuation, font fallback, and Word-exact list/table parity are not claimed |
| Metafiles | Add narrowly validated raster profiles when fixtures prove them | General WMF/EMF vector replay, composition, scaling, cropping, and mirroring remain unsupported |
| Editing | Expand package-preserving mutations where the target structure and rollback behavior are unambiguous; bounded top-level removal prunes proven-orphaned image relationships/media | Arbitrary rich block editing, nested-container mutation, general relationship garbage collection, and cross-block range rewriting remain limited |

Future changes are selected from reproducible fixtures and focused regression
tests. See [CONTRIBUTING.md](CONTRIBUTING.md) for the contributor gate and
release validation workflow.

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
