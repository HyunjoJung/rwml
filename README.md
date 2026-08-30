# rwml

**One native Rust toolkit for legacy and modern Microsoft Word files.**

Read `.doc` and `.docx` through one document model. Create styled `.docx`,
modify existing DOCX packages while preserving untouched parts, export semantic
text, and render native preview PDFs.

[![Crates.io](https://img.shields.io/crates/v/rwml.svg)](https://crates.io/crates/rwml)
[![Docs.rs](https://docs.rs/rwml/badge.svg)](https://docs.rs/rwml)
[![CI](https://github.com/HyunjoJung/rwml/actions/workflows/ci.yml/badge.svg)](https://github.com/HyunjoJung/rwml/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![MSRV](https://img.shields.io/badge/MSRV-1.85%20(render%201.92)-orange.svg)

The core library requires no JVM, Apache POI, Microsoft Office automation, or
subprocess. It handles legacy codepages, including Korean cp949, in-process and
uses bounded parsers with typed errors for malformed or unsupported input.

```sh
cargo add rwml@0.1.4
```

## Why rwml

- **One model for both Word generations.** `Document::open` detects DOC or DOCX
  from its bytes and exposes the same paragraphs, runs, tables, images, fields,
  notes, metadata, diagnostics, and export surfaces.
- **Preservation-aware DOCX editing.** Supported edits reserialize only touched
  XML or media parts. A no-op open/save retains every package-part payload
  byte-for-byte.
- **Native Rust.** Legacy OLE2/FIB/piece-table parsing and OOXML/OPC parsing run
  without Office, LibreOffice, Java, or a helper executable.
- **Explicit fidelity.** Unsupported and layout-dependent content remains
  cached or preserved with typed reasons and renderer warnings instead of being
  silently presented as fully interpreted.
- **Native preview output.** The optional renderer shapes selectable text,
  embeds subsetted fonts, handles Korean/CJK and bounded RTL content, and emits
  PDF without launching an external converter.
- **Small core dependency set.** A legacy-DOC-only build uses `cfb`,
  `encoding_rs`, and `thiserror`; DOCX and rendering are additive features.

## Format support

| Input | Read into `DocModel` | Write or convert | Preserve and edit | Export and preview |
|---|:---:|---|:---:|---|
| `.doc` (Word 97-2003) | Yes | Convert to `.docx` | No | text, Markdown, HTML, PDF preview |
| `.docx` | Yes | Create styled `.docx` | Yes | text, Markdown, HTML, PDF preview |
| `DocModel` built in Rust | Already modeled | Create styled `.docx` | Not applicable | Markdown, HTML, PDF preview |

### Common document surfaces

The source format changes the parser, not the application-facing model:

| Need | API |
|---|---|
| Searchable text | `extract_text`, `Document::text` |
| Rich structure | `Document::model`, `DocModel`, `Block`, `Paragraph`, `Table` |
| Semantic export | `Document::to_markdown`, `Document::to_html` |
| Fields and images | `Document::fields`, `Document::images` |
| Feature diagnostics | `Document::report`, `rwml diagnose` |
| Fresh authoring | `DocBuilder`, `write_docx` |
| Package-preserving edits | `Document` edit methods, `EditSession`, `save` |
| Browser inspection | `rwml::wasm`, `examples/wasm-demo/` |

## Quick start

### Read DOC or DOCX

```rust
let bytes = std::fs::read("report.docx")?;
let doc = rwml::Document::open(&bytes)?;

println!("{}", doc.to_markdown());
println!("{}", doc.report().to_json());

let model = doc.model();
for block in &model.blocks {
    // Paragraph, Table, Image, Chart, PageBreak, or SectionBreak
}
```

For plain-text indexing, use the shorter path:

```rust
let text = rwml::extract_text(&std::fs::read("legacy.doc")?)?;
```

### Create a styled DOCX

```rust
let model = rwml::DocBuilder::new()
    .title("Quarterly operations report")
    .heading(1, "Summary")
    .paragraph("Generated without Word, a JVM, or a template.")
    .numbered_list(["Open the source", "Build the model", "Write DOCX"])
    .build();

std::fs::write("report.docx", rwml::write_docx(&model))?;
```

### Edit an existing DOCX

```rust
let mut doc = rwml::Document::open(&std::fs::read("template.docx")?)?;

doc.replace_body_text("DRAFT", "FINAL")?;
doc.fill_template_fields([
    ("client-name", "Acme & Co"),
    ("report-period", "2026 Q3"),
])?;

let touched = doc.edited_parts();
std::fs::write("final.docx", doc.save()?)?;
println!("updated package parts: {touched:?}");
```

### CLI and PDF preview

```sh
cargo install rwml --version =0.1.4 --locked

rwml extract file.docx
rwml convert legacy.doc md
rwml diagnose file.docx
rwml to-docx legacy.doc converted.docx
```

PDF support is opt-in because it adds the shaping and PDF stack. The
`bundled-fonts` feature includes deterministic OFL Noto subsets for Korean and
hanja, Arabic, and Hebrew:

```sh
cargo install rwml --version =0.1.4 --locked --features bundled-fonts
rwml to-pdf file.docx preview.pdf --report-json render.json
```

The renderer is a deterministic preview/report renderer, not a Word layout
engine. See [Compatibility and limits](#compatibility-and-limits) before using
it for pagination-sensitive output.

## Preservation

Package-preserving editing is intentionally narrower than reading. An opened
DOCX keeps its OPC package and live XML trees alongside the common model.
Supported mutations update only the owning parts; unrelated fields, shapes,
content controls, comments, tracked changes, themes, custom XML, and other
unmodeled content remain in the package.

The edit surface covers focused text, field, content-control, comment, note,
image, metadata, table, hyperlink, and bounded body-block operations. Capability
checks reject edits that cannot preserve package structure. `edited_parts()`
reports the parts changed by the current document, and `EditSession` provides a
refreshing commit path for batches of supported edits.

Fresh DOCX generation is a separate path: `write_docx` serializes the supported
`DocModel`; it does not pretend to preserve unknown parts from another package.

## Validation

Version `0.1.4` is published on
[crates.io](https://crates.io/crates/rwml/0.1.4) and
[docs.rs](https://docs.rs/rwml/0.1.4/rwml/). Its exact crates, checksums,
public-hygiene result, extraction benchmark, render validation, and strict
revision-bound manifest are attached to the
[`v0.1.4` release](https://github.com/HyunjoJung/rwml/releases/tag/v0.1.4).

| Release | Safety and toolchain | Public release evidence |
|---|---|---|
| `0.1.4` / MIT | no `unsafe`; core MSRV 1.85; render MSRV 1.92 | 21 DOCX fixtures, 26 rendered pages, 3 generated DOC oracle fixtures, tag-bound package manifest |

The release contract uses only redistributable public inputs:

| Gate | Public input | Release check |
|---|---|---|
| DOCX parsing and preservation | 21 generated or permissively licensed documents | expected diagnostics, open/save part stability, bounded edit validation |
| Legacy DOC extraction | 3 generated Word 97-2003 documents | exact Apache POI 5.2.3 and LibreOffice 26.2.3.2 text oracles |
| PDF preview | the same 21-document manifest, 26 pages | text recall, page counts, visual summaries, zero skipped inputs |
| Public hygiene | source tree plus bounded Office-package inspection | filenames, metadata, text parts, corpus provenance, and license-clean inputs |

Inputs and provenance live under [`corpus/public/`](corpus/public). Run the fast
public checks with:

```sh
python3 scripts/gen_public_corpus.py --check
cargo test --test public_corpus
cargo test --features render --test public_corpus
```

Pull-request CI adds the feature/MSRV matrix, formatting, strict Clippy,
documentation, dependency audit, fuzz-target build, WASM smoke tests, and
Python release-tooling tests. Tag-only release validation adds release-mode
performance, external extraction/render oracles, exact package preflight, and
manifest verification. See
[CONTRIBUTING.md](CONTRIBUTING.md#validation-by-change-area) for the commands.

## Architecture

```text
 .doc  -- OLE2 / FIB / piece table --\
                                      +--> DocModel --> text / Markdown / HTML
 .docx -- OPC / WordprocessingML ----/            \--> fresh DOCX / PDF preview
        \-- retained package + live XML ----------------> bounded DOCX edits
```

**DOC** parsing navigates the compound file, FIB, piece table, codepages,
formatting bins, styles, lists, tables, sections, fields, notes, annotations,
and images before assembling the shared model.

**DOCX** parsing walks the OPC package and WordprocessingML parts for body
content, styles, numbering, relationships, notes, comments, running surfaces,
fields, tables, drawings, revisions, charts, and supported metadata. The
retained package remains available for preservation-aware editing.

**Rendering** flows the model through deterministic section, column, paragraph,
list, table, image, chart, bidi, font-shaping, and page-placement stages before
emitting PDF. **Writing** serializes a fresh model into OOXML. Opening and saving
an existing DOCX uses the preservation path instead of regenerating that package
from the lossy common model.

## Documentation

| Resource | Contents |
|---|---|
| [docs.rs](https://docs.rs/rwml/latest/rwml/) | Public Rust API and feature-gated surfaces |
| [`examples/`](examples) | Read, write, convert, edit, diagnose, render, and WASM programs |
| [`corpus/public/`](corpus/public) | License-clean fixtures, manifests, oracles, and provenance |
| [CHANGELOG.md](CHANGELOG.md) | Release history and compatibility changes |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Public workflow, validation matrix, fixtures, and release preflight |
| [Security policy](.github/SECURITY.md) | Private vulnerability reporting |

## Features and status

| Cargo feature | Default | Surface |
|---|:---:|---|
| `docx` | Yes | DOCX read/write, conversion, CLI, and package-preserving editing |
| `render` | No | Native PDF rendering with `parley` and `krilla`; MSRV 1.92 |
| `bundled-fonts` | No | `render` plus OFL Noto subsets for Korean/hanja, Arabic, and Hebrew |

Use `default-features = false` for the dependency-light legacy DOC reader:

```toml
rwml = { version = "0.1.4", default-features = false }
```

### Built-in surfaces

- **Readers:** paragraphs, rich runs, styles, lists, tables, sections, notes,
  comments, revisions, fields, hyperlinks, images, charts, metadata, and
  diagnostic sidecars where the source format exposes them.
- **Field handling:** deterministic evaluation for a documented bounded subset;
  unsupported, external-state, and layout-dependent results preserve cached
  display text with typed fallback reasons.
- **DOCX authoring:** styled text, lists, tables, links, comments, notes,
  revisions, fields, content controls, images, charts, sections, running
  surfaces, and metadata represented by the public model/builders.
- **Export and diagnostics:** text, Markdown, HTML, image extraction, and
  machine-readable feature/report JSON.
- **Portable interfaces:** native library and CLI plus a thin WASM read/report
  adapter and browser inspector example.

## Compatibility and limits

`rwml` distinguishes interpreted behavior, preserved content, and preview
approximations:

| Area | Current contract | Important boundary |
|---|---|---|
| Legacy `.doc` | read, inspect, export, convert, preview | no in-place editing or DOC writer; Word 6/95 and encrypted files are rejected |
| `.docx` read | rich model plus diagnostics for major WordprocessingML surfaces | not every producer extension or layout-dependent field can be interpreted |
| Fresh `.docx` | styled model-backed document generation | output contains supported model content, not unknown parts from a source package |
| Package-preserving edit | focused, capability-checked mutations with untouched-part retention | no generic XML/DOM editor or silent package regeneration |
| PDF | selectable-text preview with page geometry, styles, lists, tables, images, charts, links, and bounded floating-shape hints | not Word-exact pagination, general floating-object reflow, or complete Office-Art rendering |
| WASM | extraction, Markdown/HTML, and diagnostics | the browser example is an inspector, not an editing UI |

Unknown DOCX parts remain in a safe retained package. Unsupported metafiles,
floating shapes, embedded objects, chart forms, and layout-dependent fields may
produce diagnostics, cached text, raster fallbacks, or preview placeholders. For
Word-exact or archival PDF conversion, use Word or LibreOffice as the renderer.

## Roadmap

Work is selected from public, reproducible inputs and focused regression tests:

- deepen bounded DOC/DOCX parsing and deterministic field evaluation;
- expand preservation-safe edits only where rollback and ownership are clear;
- improve model-backed PDF layout, complex scripts, tables, images, and sections;
- add license-clean producer fixtures and mature-tool oracles.

Values requiring Word layout or external state remain cached with a reason.
There is no planned generic DOM editor or promise of Word-exact pagination.
Open an [issue](https://github.com/HyunjoJung/rwml/issues) with a minimal public
file or structural reproducer when a real document exposes a missing case.

## Contributing

Issues and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) defines
the focused topic-branch workflow, red-test expectation, validation matrix,
public-fixture rules, release preflight, and optional AI/BMad guidance. Every
change must be understandable from public code, tests, issues, and pull-request
context; never upload a private document or planning artifact.

See also the [Code of Conduct](.github/CODE_OF_CONDUCT.md) and
[Security policy](.github/SECURITY.md).

## License

Licensed under the [MIT License](LICENSE). Third-party dependency licenses are
listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md). Bundled font
subsets retain their upstream OFL licenses and provenance under
[`rwml-fonts/`](rwml-fonts).

## Trademarks

`rwml` takes its name from WordprocessingML, the ECMA-376 markup for
word-processing documents. It is an independent open-source project, not
affiliated with, authorized by, or endorsed by Microsoft. Microsoft, Microsoft
Word, and the `.doc` / `.docx` file formats are trademarks or registered
trademarks of Microsoft Corporation, referenced only to indicate compatibility.

The implementation is based on publicly documented [MS-DOC], [MS-CFB], and
[ECMA-376] specifications and contains no Microsoft source code.

[MS-DOC]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/
[MS-CFB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/
[ECMA-376]: https://ecma-international.org/publications-and-standards/standards/ecma-376/
