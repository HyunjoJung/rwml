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
 (build it yourself) ┘             └→ render_pdf   (typeset PDF)
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
            text: "입찰 비교 리포트".into(),
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

## Render — typeset to PDF

Lay any model out to a paginated A4 **PDF** with native typesetting — `parley`
shapes and line-breaks (Korean/CJK [UAX #14] line-breaking + script font fallback),
`krilla` emits the PDF with subsetted embedded fonts and **selectable text**. Rich
runs (color/size/font), lists and indentation, bordered tables with shaded,
vertically-aligned cells and authored column widths, images, and **clickable
hyperlink annotations** are drawn; multi-page tables repeat their header rows and a
row taller than a page splits across pages. Behind the `render` feature.

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
| `docx`   | ✅ | `zip`, `quick-xml` | `.docx` read **and** `write_docx` |
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
`word/_rels/document.xml.rels` + `word/media/*`. Recursion is depth-capped, XML
external entities are never resolved (XXE-safe), and per-entry decompression is
size-capped (zip-bomb guard).

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
the full population of real Korean government `.doc` attachments at ~97.4%
whitespace-insensitive parity vs POI (body text ~100%). `.docx` validated against
**python-docx** on the 127-file Apache POI corpus (87 with comparable text):
**98.6% mean word recall, 100% median, 85/87 ≥ 99%**, whole corpus panic-free
(incl. clusterfuzz inputs). The two sub-99% files are a duplicate-`document.xml`
zip-confusion container and a heavy tracked-changes doc (accept-changes view) —
explained, not extraction bugs.

**Writing.** `read → model → write_docx → read` round-trips structure and the rich
character/paragraph/table formatting (covered by unit + integration tests); the
authored report opens in Word and LibreOffice.

**Rendering.** [`scripts/render_validate.py`](scripts/render_validate.py) compares
the renderer to LibreOffice per document on three metrics (text recall, page-count
ratio, average-hash visual similarity). On real corpus `.docx`, page counts match
and pages are visually close; **what the model contains renders faithfully**.
End-to-end text recall *against LibreOffice* is bounded not by the typesetter but by
what the **reader** currently lifts into the model — headers/footers and text inside
drawings/shapes are not yet extracted, so LibreOffice shows text rdoc's model never
received. That is a reader-coverage limit, surfaced honestly by the metric.

**Still out of scope, honestly:**

- *Both formats (read):* metafile images (WMF/EMF), OLE-embedded objects and
  floating Office-Art shapes (placeholders only); fields' *computed* values
  (cached result text is kept); encrypted files (detected and rejected).
- *`.doc` read only:* per-instance list overrides (`LFOLVL` start-at); Word 6/95.
- *`.docx` read only:* headers/footers, footnotes/endnotes, comments, and text-box
  parts (so `main_text()` == `text()`); style-*inherited* emphasis (only direct
  `w:rPr` is read, matching the `.doc` CHPX behavior).
- *Render:* a single fixed A4 page geometry (model page setup drives the `.docx`
  writer, not yet the renderer); right-to-left scripts; no embedded CJK font is
  bundled into the crate — install a system CJK font or pass one to
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
- [ ] Reader: `.docx` headers/footers, footnotes, comments, text-box parts
- [ ] Renderer: page geometry from model setup; bundled-font feature; RTL
- [ ] Metafile (WMF/EMF) inflate; floating Office-Art shapes; field semantics

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
