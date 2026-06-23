# rdoc

[![Crates.io](https://img.shields.io/crates/v/rdoc.svg)](https://crates.io/crates/rdoc)
[![Docs.rs](https://docs.rs/rdoc/badge.svg)](https://docs.rs/rdoc)
[![CI](https://github.com/HyunjoJung/rdoc/actions/workflows/ci.yml/badge.svg)](https://github.com/HyunjoJung/rdoc/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![MSRV](https://img.shields.io/badge/MSRV-1.74%20(render%201.88)-orange.svg)

A native Rust toolkit for Microsoft Word documents — **read**, **write**, and
**render** — covering **both** formats: legacy **`.doc`** (Word 97–2003 binary,
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
let text = rdoc::extract_text(&bytes)?;

// Rich model + exporters (identical IR for .doc and .docx):
let doc   = rdoc::Document::open(&bytes)?;
let md    = doc.to_markdown();   // # headings, **bold**, | tables |, lists, links
let html  = doc.to_html();       // <h1>, <strong>, <table colspan>, <img>, <a>
let model = doc.model();         // typed IR: Vec<Block> (Paragraph | Table | Image)
let imgs  = doc.images();        // extracted PNG/JPEG bytes (like POI getAllPictures)
```

## Write — author a styled `.docx`

Build a [`DocModel`] from your data and serialize it to a clean, Office-openable
`.docx`. Character formatting (font, size, color, bold/italic, highlight,
super/subscript), paragraph styles (named headings, alignment, spacing, indent,
shading), lists, **bordered tables with per-cell shading / width / vertical
alignment**, images, page setup, and running headers/footers with page numbers all
round-trip. See [`examples/report.rs`](examples/report.rs).

```rust
use rdoc::{Block, DocModel, Paragraph, ParaProps, Run, CharProps, Color};

let model = DocModel {
    blocks: vec![Block::Paragraph(Paragraph {
        props: ParaProps { heading_level: Some(1), ..Default::default() },
        runs: vec![Run {
            text: "분기 운영 리포트".into(),
            props: CharProps { color: Some(Color { r: 0x1F, g: 0x38, b: 0x64 }), ..Default::default() },
            ..Default::default()
        }],
    })],
    ..Default::default()
};
std::fs::write("out.docx", rdoc::write_docx(&model))?;   // opens in Word & LibreOffice
```

The output is validated to re-open in **Word** (verified via python-docx reading
back the named styles, run colors, fonts, and table shading) and **LibreOffice**.

## Edit — open, change, save (package-preserving)

`Document::open` keeps the whole package, so `save()` re-emits it with everything
rdoc doesn't model preserved verbatim (themes, settings, fonts, comments, custom
XML, charts, embeddings, unknown parts). A no-op open→save is byte-stable per part.

```rust
let mut doc = rdoc::Document::open(&std::fs::read("in.docx")?)?;

// Element-tree edit: preserves fields, content controls, shapes, comments…
doc.replace_body_text("DRAFT", "FINAL")?;
doc.add_image_png(&png_bytes, "image1.png")?;   // media + content-type + rId, atomic

std::fs::write("out.docx", doc.save()?)?;        // only document.xml changed
```

Edits mutate the live `document.xml` **element tree** in place
(`replace_body_text` / `add_image_png`), so everything they don't touch — including
content the lossy model can't represent (fields, content controls, shapes, comments,
tracked changes) — is preserved byte-for-byte; `save()` re-serializes only the parts
you changed. `Document::new()` starts from a bundled blank template. To *author* a
document from data (or convert a `.doc`), build a `DocModel` and use
[`write_docx`](#author--build-a-styled-docx) instead.

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
> match LibreOffice fidelity — exact pagination, floating-shape positioning, field
> page numbers, and pixel layout differ. Measured against LibreOffice on a real
> corpus it reaches ~0.93 text recall with close page counts; for archival or
> Word-exact PDF, render via LibreOffice. (See *Scope & parity*.)

```toml
rdoc = { version = "0.1", features = ["render"] }
```

```rust
let pdf = rdoc::render_pdf(&model);                 // uses system fonts
// On a headless host without CJK fonts, supply your own:
let kr  = std::fs::read("NotoSansKR-Regular.otf")?;
let pdf = rdoc::render_pdf_with_fonts(&model, &[kr]);
```

You can also convert a parsed document straight to PDF: `Document::open(&bytes)?.to_pdf()`.

## CLI (examples)

```text
cargo run --example extract  -- file.docx              # plain text
cargo run --example convert  -- file.doc  md           # Markdown / html
cargo run --example to_docx  -- legacy.doc out.docx    # .doc → clean .docx
cargo run --example report   -- report.docx            # author a styled report
cargo run --features render --example to_pdf -- file.docx out.pdf
```

## Cargo features

| feature | default | pulls in | enables |
|---|:--:|---|---|
| `docx`   | ✅ | `zip`, `quick-xml` | `.docx` read, `write_docx`, **and package-preserving edit/`save`** |
| `render` |    | `parley`, `krilla` | `render_pdf` / `to_pdf` (MSRV 1.88) |

For a dependency-light, legacy-only build (just `cfb` + `encoding_rs` +
`thiserror`): `rdoc = { version = "0.1", default-features = false }` (reads `.doc`,
emits text/markdown/html).

## Why one crate? (and how this relates to `docx-rs`)

The mature [`docx-rs`](https://crates.io/crates/docx-rs) reads and writes `.docx`
well, and for a `.docx`-only need it is the obvious choice. `rdoc` adds `.docx`
**not** to beat it on `.docx` features, but for **unification**: legacy `.doc` (no
comparable pure-Rust option exists) and `.docx` produce the *identical* [`DocModel`]
and share one read/write/render surface, with no JVM and no second Word parser in
the tree. If you only ever touch `.docx`, prefer `docx-rs`.

## How it works

A `.docx` is a ZIP of XML parts. `rdoc` reads `word/document.xml` with `quick-xml`
by recursive descent (paragraphs → runs with `w:rPr`; tables `w:tbl` with
`gridSpan`/`vMerge` → real colspan/rowspan), resolves heading levels from
`word/styles.xml` (`w:pStyle` / `Heading N` / `제목 N`), ordered-vs-bullet from
`word/numbering.xml`, and hyperlink targets + image bytes from
`word/_rels/document.xml.rels` + `word/media/*`. Running headers/footers are
resolved from the `sectPr` references (`word/header*.xml` / `footer*.xml`, each
with its own rels) into `DocSetup`, and text-box text (`w:txbxContent`, DrawingML
or VML, single-branch on `mc:AlternateContent`) is folded back into the body.
Recursion is depth-capped, XML external entities are never resolved (XXE-safe), and
per-entry decompression is size-capped (zip-bomb guard).

`.doc` is an OLE2 compound file. `rdoc` opens it with `cfb`, parses the **FIB** by
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
Edits go through the live `document.xml` **element tree** (`replace_body_text` /
`add_image_png`), so unmodeled body content (fields, shapes, content controls,
comments, tracked changes) survives. Validated on the 127-file corpus with python-docx
as the strict external checker: passthrough is part-payload byte-stable; the
element-tree image insert produces a package python-docx opens with the inline image
present on every openable file; both fail cleanly (no panic) on a pathologically-deep
file and a structurally-broken original. To author/convert from a `DocModel`, use
`write_docx` (it regenerates a fresh package, lossy w.r.t. unmodeled content). See
`docs/prd-rdoc-write-edit.md` / `docs/trd-rdoc-write-edit.md`.

**Rendering.** [`scripts/render_validate.py`](scripts/render_validate.py) compares
the renderer to LibreOffice per document on three metrics (text recall, page-count
ratio, average-hash visual similarity). rdoc is a **preview-grade**
renderer, faithful to the model but **not** a LibreOffice replacement. On a real
`.docx` corpus it reaches **~0.93 mean text recall** (extracting headers/footers,
text boxes, nested tables, real list labels, caps; model-driven page geometry makes
`.doc` page counts line up — mean `.doc` render recall ~0.96). It still trails
LibreOffice on exact pagination, floating-shape placement, computed field/page
numbers, and pixel-level visual fidelity; those are inherent to a compact native
renderer, not bugs to be closed to parity. For Word-exact or archival PDF, render
via LibreOffice. rdoc aims to match specialist extractors on text/model recovery
while staying dependency-light; render fidelity remains below LibreOffice.

**Still out of scope:**

- *Both formats (read):* metafile images (WMF/EMF), OLE-embedded objects and
  floating Office-Art shapes (placeholders only); fields' *computed* values
  (cached result text is kept; instruction text may surface for some fields like
  `PAGE`/`TOC`); symbol-font (Symbol/Wingdings) glyph mapping; encrypted files
  (detected and rejected).
- *`.doc` read only:* per-instance list overrides (`LFOLVL` start-at); Word 6/95.
  Header/footnote/text-box text appears in `text()` but the `.doc` *model* still
  flattens it into the body (no `DocSetup.header`/`footer` split yet).
- *`.docx` read only:* comments and tracked-change *original* views; style-*inherited*
  emphasis (only direct `w:rPr` is read, matching the `.doc` CHPX behavior).
  Headers/footers, text boxes, footnotes/endnotes, and per-level numbering labels
  **are** now extracted; `text()` includes headers/footers, `main_text()` is body-only.
- *Write/edit:* rdoc now **does** preserve arbitrary OOXML parts when editing an
  opened `.docx` (`save()` keeps comments, revisions, charts, content controls, custom
  XML, themes, fonts verbatim). The remaining gaps are *authoring* APIs for those
  constructs (no high-level builder for comments/revisions/charts yet — they survive
  but rdoc won't create them), and a *structural* editing surface (the element-tree
  edit exposes text-replace + image-insert; richer in-place mutations are future work).
- *Render:* preview-grade vs LibreOffice (see above); right-to-left scripts; no
  embedded CJK font is bundled — install a system CJK font or pass one to
  `render_pdf_with_fonts`.

## Roadmap

- [x] Codepage-aware `.doc` text; encryption / Word 6/95 detection gates
- [x] Full read model: runs (CHPX incl. font/size/color), headings (STSH), tables
      (`sprmTDefTable`), list autonumbers, hyperlinks, inline images
- [x] Unified `.docx` reader into the same model (98.6% recall vs python-docx)
- [x] **`.docx` writer** — styled authoring (named styles, rich tables, page setup,
      headers/footers + page numbers, images) via `write_docx`
- [x] **PDF renderer** — `parley` + `krilla` with rich text/tables/images/lists/
      hyperlinks, header-row repeat, oversized-row split, font registration
- [x] Reader: `.docx` headers/footers, text boxes (`w:txbxContent` incl. run-level
      `mc:AlternateContent`), footnotes/endnotes, per-level numbering labels, caps
- [x] Renderer: model-driven page geometry (size/orientation/per-side margins);
      running headers/footers; nested-table-cell text
- [ ] Reader: `.docx` comments & tracked-change views; field evaluation
      (`PAGE`/`TOC`/`FILENAME`); Symbol/Wingdings glyph mapping; `.doc` header/
      text-box model split (currently flattened into the body)
- [x] **Package-preserving edit layer** — `Document::open`→edit→`save` keeps every
      unmodeled part verbatim; element-tree edits (`replace_body_text`,
      `add_image_png`) preserve fields/shapes/content-controls/comments; `opc` +
      `xmltree` internals; fallible `try_write_docx`
- [ ] Renderer: exact pagination & floating-shape placement; field page numbers;
      bundled-font feature; RTL
- [ ] Authoring APIs for comments/revisions/charts (they round-trip on edit today,
      but rdoc can't yet *create* them); metafile (WMF/EMF) inflate; `try_render_pdf`

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The local gate is `cargo fmt --all -- --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`,
`cargo doc --no-deps`.

## License

Licensed under the [MIT License](LICENSE). Third-party dependency licenses are
listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md). This crate implements
only the publicly documented [MS-DOC] / [MS-CFB] / OOXML specifications and contains
no Microsoft source.

[MS-DOC]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/
[MS-CFB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/
[UAX #14]: https://unicode.org/reports/tr14/
