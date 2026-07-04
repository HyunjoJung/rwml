# rwml

[![Crates.io](https://img.shields.io/crates/v/rwml.svg)](https://crates.io/crates/rwml)
[![Docs.rs](https://docs.rs/rwml/badge.svg)](https://docs.rs/rwml)
[![CI](https://github.com/HyunjoJung/rwml/actions/workflows/ci.yml/badge.svg)](https://github.com/HyunjoJung/rwml/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![MSRV](https://img.shields.io/badge/MSRV-1.85%20(render%201.92)-orange.svg)

**rwml** — from **W**ord**p**rocessing**ML**, the OOXML markup for Word documents
— is a native Rust toolkit for Microsoft Word documents: **read**, **write**,
**edit**, and **render**, covering **both** formats: legacy **`.doc`** (Word 97–2003 binary,
[MS-DOC]) and modern **`.docx`** (OOXML WordprocessingML). No JVM, no Apache POI,
no other `.docx` crate, no subprocess.

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

Every one of the edit methods above mutates live WordprocessingML **element
trees** or media parts in place, so everything they don't touch — including
content the lossy model can't represent (fields, content controls, shapes,
comments, tracked changes) — is preserved byte-for-byte; `save()` re-serializes
only the parts you changed.
Regenerated relationship parts are validated before save, so internal
relationship targets must point at retained package parts unless they are
explicitly external.
`Document::new()` starts from a bundled
blank template. To *author* a
document from data (or convert a `.doc`), build a `DocModel` and use
[`write_docx`](#author--build-a-styled-docx) instead.
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
runs (color/size/font, caps/small-caps), lists with real autonumber labels and
indentation, bordered tables with shaded, vertically-aligned cells and authored
column widths, images, and **clickable hyperlink annotations** are drawn; page
size/orientation and per-side margins come from the document; multi-page tables
repeat their header rows and a row taller than a page splits across pages. Behind
the `render` feature.

> **Scope:** this is a fast, in-process **preview / report** renderer, not a Word
> layout engine. It is faithful to the *model* and selectable, but it does **not**
> match LibreOffice fidelity — exact pagination, exact floating-object layout,
> unknown fields, unresolved or unsupported remaining value-changing REF cases
> beyond the deterministic note/comment-reference mark subset,
> remaining advanced
> TOC/REF computed field evaluation, and pixel layout differ. Opened-document
> renders draw bounded approximate overlay boxes for recovered `.docx`
> floating-shape geometry on the recovered top-level body block page when
> available, and compact placeholder lines for preserved charts,
> OLE objects, WMF/EMF/EMZ/WMZ images, image nodes whose bytes are unavailable,
> raster images the PDF backend cannot decode, and any floating-shape markers
> without recovered geometry rather than drawing those objects exactly.
> Measured against LibreOffice on a real corpus it reaches ~0.93 text recall with
> close page counts; for archival or Word-exact PDF, render via LibreOffice.
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

For portable Korean PDF rendering, enable `bundled-fonts` to opt into the separate OFL-licensed `rwml-fonts` crate: it registers a Noto Sans KR subset covering KS X 1001 Hangul + hanja plus Latin while `rwml` itself remains MIT-licensed. Other scripts still use system font fallback, matching `render_pdf_with_fonts`.

`layout_pages_with_fonts` exposes **layout-derived page numbers** from the same
preview-grade pagination: the page count plus the page each body `PAGE` field
and top-level block lands on — matching rwml's own PDF output, **not**
Microsoft Word's pagination. Supplied fonts are used strictly (system fonts
disabled), so identical document + font bytes give identical pages; results
live in a separate `LayoutPages` record and never overwrite reader-path
`computed_result` semantics.

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
python scripts/render_validate.py --json --min-mean-recall 0.90 --max-skipped 0 corpus/public/**/*.docx > render.json
python scripts/bench_vs_mature.py --corpus "$RWML_BENCH_CORPUS" --json \
  --version 0.1.0 --git-rev "$(git rev-parse HEAD)" \
  --min-poi-recall-mean 0.95 --min-poi-f1-mean 0.95 --max-errors 0 --min-scored 1 \
  --output dist/extract-benchmark.json
python scripts/public_hygiene_audit.py --json > dist/public-hygiene.json
python scripts/release_manifest.py --version 0.1.0 --git-rev "$(git rev-parse HEAD)" \
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
| `bundled-fonts` |    | `render`, `rwml-fonts` | `render_pdf_bundled` with an OFL Noto Sans KR subset covering KS X 1001 Hangul + hanja |

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
`word/styles.xml` (`w:pStyle` / `Heading N` / `제목 N`), ordered-vs-bullet from
`word/numbering.xml`, and hyperlink targets + image bytes from
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
underline/strike, **font name from `SttbfFfn`, half-point size, color**), the STSH
style sheet + outline levels (headings), `sprmTDefTable` (merge-aware tables), list
autonumbers, hyperlink field marks, and `PICF` inline images.

The `.docx` **writer** is the inverse of the reader, part by part: `document.xml`
(`w:rPr`/`w:pPr` with the full property set), a synthesized `styles.xml`
(Normal + Heading1–6 with `outlineLvl`), `numbering.xml`, header/footer parts wired
through `sectPr`, media parts + relationships for images, and external relationships
for hyperlinks. The **renderer** flows the model into A4 pages and draws each page's
glyph runs, table grids, shading, and images with krilla.

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
the renderer to LibreOffice per document on three metrics (text recall, page-count
ratio, average-hash visual similarity) plus rwml render-warning counts/kinds, and
can emit a JSON report for release tracking. Its `--soffice auto` default uses a
local `soffice` when available and otherwise falls back to the Docker `lo-cli`
backend. The public synthetic corpus also includes a render manifest checked by
`cargo test --features render`. rwml is a **preview-grade**
renderer, faithful to the model but **not** a LibreOffice replacement. Generated
running footer page numbers and body `PAGE` fields are computed from the emitted
PDF page list; section-aware default/first/even running header/footer variants
are selected with first-page variants scoped to each section and even variants
based on emitted page parity; field-code `HYPERLINK` runs render as link
annotations for target/anchor, tooltip/frame, and documented `\m`/`\n` no-op
switch tails, and malformed hyperlink syntax reports `UnsupportedSwitch`.

**Field evaluation** is deterministic and source-order stable, and applies
identically in the reader, the render model, and side-table text surfaces
(comment bodies/anchors, tracked-change text, note anchors, floating-shape and
text-box text, TOC heading sources). Every `FieldKind` is distinguished from an
unknown field; where a value can't be computed deterministically the cached
result text is preserved (including inline tabs, line breaks, and no-break/soft
hyphens for simple and common complex fields) and a reason is reported. The four
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
`.docx` corpus it reaches **~0.93 mean text recall** (extracting headers/footers,
text boxes, nested tables, real list labels, caps; model-driven page geometry makes
`.doc` page counts line up — mean `.doc` render recall ~0.96). It still trails
LibreOffice on exact pagination, exact floating-object layout, remaining
layout-derived `PAGEREF` page-reference computation beyond trusted source markers,
advanced TOC/REF/NOTEREF computed fields, and
pixel-level visual fidelity; those are inherent to a compact native
renderer, not bugs to be closed to parity. For Word-exact or archival PDF, render
via LibreOffice. rwml aims to match specialist extractors on text/model recovery
while staying dependency-light; render fidelity remains below LibreOffice.
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
body-page anchoring beyond that best-effort block page, real text-wrap reflow,
and non-text Office-Art drawing contents remain out of scope.
[`scripts/bench_vs_mature.py`](scripts/bench_vs_mature.py) emits a schema-tagged
JSON extraction benchmark report against local Apache POI and LibreOffice
goldens and can enforce release thresholds for mean POI recall/F1, mean
LibreOffice recall, scored-file counts, and extractor errors. Render-validation
JSON also carries a compact `gate` section for per-document recall plus optional
mean recall, page-ratio, aHash, warning, and skipped-file thresholds. Release
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

- *Both formats (read/render):* metafile images (WMF/EMF/EMZ/WMZ), OLE-embedded objects,
  and exact floating Office-Art layout (`.docx` `wp:anchor` geometry, z-order
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
  Word-exact anchored/wrapped Office-Art content; metafile metadata is exposed
  in diagnostics with bounded header inflation, and a single full-frame embedded
  DIB wrapped in a metafile is extracted and rendered as a raster image, but
  general vector metafile payloads are not rendered);
  unknown or broader fields' *computed* values
  (cached result text is kept, including inline tabs, line breaks, and
  no-break/soft hyphens for simple and common complex body fields; `.docx`
  REF/TOC cases listed above plus recovered `.doc` field instructions are
  exposed through `fields()`/diagnostics);
  complete symbol-font (Symbol/Wingdings) glyph coverage beyond the common
  deterministic mapped subset; encrypted files
  (detected and rejected).
- *`.doc` read only:* per-instance list overrides (`LFOLVL` start-at); Word 6/95.
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
  do not duplicate side-table fields; style-*inherited* emphasis (only direct
  `w:rPr` is read,
  matching the `.doc` CHPX behavior).
  Headers/footers, text boxes, footnotes/endnotes, and per-level numbering labels
  **are** now extracted; `header_footers()` exposes `.docx` referenced
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
  above). The remaining gaps are broader *structural* editing (the element-tree
  edit covers focused text, template/content-control filling, revision
  acceptance, field/comment/image/note operations, and `gridSpan`/`vMerge`-aware
  cell replacement — not arbitrary block restructuring) and newer extension chart
  families beyond the current authored set.
- *Render:* preview-grade vs LibreOffice (see above); right-to-left scripts; no
  embedded CJK font is bundled - install a system CJK font or pass one to
  `render_pdf_with_fonts`.

## Roadmap

The long-term native Word engine roadmap is summarized below.

Current maturity work is concentrated in deeper compatibility rather than new
top-level APIs. The remaining native Word-engine work is tracked as bounded R2
sub-buckets: field report/evaluator parity, layout-derived `PAGE`/`PAGEREF`,
remaining `REF`/`NOTEREF`/`FTNREF`/TOC policy, non-deterministic field families
that stay cached/reportable, and legacy `.doc` anchors/header-footer behavior.
Each slice should move only after focused reader/report evidence proves either
deterministic computation or precise cached-result diagnostics.

- [x] Codepage-aware `.doc` text; encryption / Word 6/95 detection gates
- [x] Full read model: runs (CHPX incl. font/size/color), headings (STSH), tables
      (`sprmTDefTable`), list autonumbers, hyperlinks, inline images
- [x] Unified `.docx` reader into the same model (98.6% recall vs python-docx)
- [x] **`.docx` writer** - styled authoring (named styles, rich tables with typed nested cell blocks, page setup,
      styled runs, leveled lists, paragraph page-break-before, simple fields, `PAGEREF` helper runs, dirty TOC heading-range fields,
      run-anchored comments with reply parent ids and commentsExtended metadata, tracked insertion/deletion runs,
      run-level content controls with data-binding metadata, bookmarked runs, authored footnotes/endnotes, inline/standalone hyperlinks,
      string custom document properties, raw custom XML data-store items, explicit Word document ids, web-extension task pane package shells, styled default/first/even headers/footers + page numbers, section columns, document grids, text direction, title pages, page-number restarts/formats, next/even/odd section breaks, images with inline rotation and page-relative floating offsets,
      table width, fixed-layout tables, table alignment, indentation, uniform/per-side border widths, styles, and colors, per-cell table margins,
      and the [core OOXML chart families](#chart-families) with embedded workbook-backed data) via `DocBuilder`,
      `ParagraphBuilder`, `RunBuilder`, `CommentBuilder`, `RevisionBuilder`,
      `ContentControlBuilder`, `TableBuilder`, `CellBuilder`, `ImageBuilder`,
      `ChartBuilder`, `DocModel`, and
      `write_docx`
- [x] **PDF renderer** - `parley` + `krilla` with rich text/tables/images/lists/
      hyperlinks, paragraph page-break-before, header-row repeat, oversized-row split, font registration
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
- [ ] Reader R2-a: field report/evaluator parity for value-changing fields
      where duplicated syntax checks or document-report/render-report
      diagnostics can drift from computed-result behavior. Verified parity
      coverage now locks `PAGEREF`, `REF`, `NOTEREF`/`FTNREF`, and TOC
      computed/gap buckets across opened-document and render-model reports, and
      empty unsupported simple/complex field instructions plus supported hidden
      `RD`/`TA`/`XE` marker fields stay reportable in model/render inventories;
      the remaining R2-a work is limited to newly proven parser/evaluator/report
      drift or exact duplicated syntax logic.
- [ ] Reader R2-b: layout-derived `PAGE`/`PAGEREF` current-page,
      page-number, and relative-position computation beyond trusted
      leading/source-rendered, section-start, paragraph-end section-break
      target, source-marker, and hard-break contexts
- [x] Reader R2-c: deterministic value-changing `REF` (incl. `\#` numeric
      picture and `\!`), `NOTEREF`/`FTNREF` (incl. `settings.xml`
      `numStart`/`numFmt` and `customMarkFollows`), and TOC heading-source
      `NOTEREF`/`SEQ` resolution; the remaining REF/NOTEREF/TOC cases are
      layout- or Word-behavior-dependent and stay cached-with-reason
- [ ] Reader R2-d: non-deterministic data-, source-, layout-, action-,
      generated-, barcode-, compatibility-, and protected-form field families
      that preserve cached text and stay reportable until deterministic
      semantics are proven
- [ ] Reader R2-e: exact legacy `.doc` note/text-box/body anchors and richer
      legacy multi-section header/footer application semantics beyond
      recovered global default/first/even running stories
- [x] **Package-preserving edit layer** — `Document::open`→edit→`save` keeps every
      unmodeled part verbatim; the element-tree edit methods (text/field/comment/
      note/image/content-control/revision/core-property, listed under
      [Edit](#edit--open-change-save-package-preserving)) preserve fields/shapes/
      content-controls/comments/revisions;
      `edited_parts` exposes touched package parts; `edit_capability` /
      `report().edit` expose read-only reasons; `opc` + `xmltree` internals;
      fallible `try_write_docx`
- [ ] Renderer: exact pagination, floating-shape page anchoring/wrap reflow,
      full layout-derived `PAGE`/`PAGEREF` values beyond trusted source markers,
      remaining render-time TOC/REF/NOTEREF policy where layout context is
      required, bundled-font feature, and RTL
- [x] Authoring API, native PDF preview rendering, and embedded workbook-backed
      data for the [core OOXML chart families](#chart-families)
- [x] Wireframe styling for authored surface and 3-D surface charts
- [x] Shape styling for authored 3-D bar and 3-D column-family charts
- [x] Metafile diagnostics for WMF/EMF/EMZ/WMZ path, format, byte size, compression flag, and raw/gzip-wrapped header dimensions
- [x] Chart-ex extension chart families (waterfall, treemap, sunburst, histogram, box & whisker, funnel) authored as `chartEx` parts
- [x] Single-DIB-wrapper metafile (WMF/EMF) raster extraction rendered as images
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

`rwml` — from **W**ord**p**rocessing**ML**, the ECMA-376 markup for
word-processing documents — is an independent open-source project, **not**
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
