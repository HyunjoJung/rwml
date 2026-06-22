# rdoc

[![Crates.io](https://img.shields.io/crates/v/rdoc.svg)](https://crates.io/crates/rdoc)
[![Docs.rs](https://docs.rs/rdoc/badge.svg)](https://docs.rs/rdoc)
[![CI](https://github.com/HyunjoJung/rdoc/actions/workflows/ci.yml/badge.svg)](https://github.com/HyunjoJung/rdoc/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![MSRV](https://img.shields.io/badge/MSRV-1.74-orange.svg)

One native Rust reader for **both** Microsoft Word formats — legacy **`.doc`**
(Word 97–2003 binary, [MS-DOC]) and modern **`.docx`** (OOXML WordprocessingML).
No JVM, no Apache POI, no external `.docx` crate, no subprocess. [`Document::open`]
detects the format from the magic bytes (OLE2 `D0CF11E0` → `.doc`; ZIP `PK` →
`.docx`) and both feed the **same** document model and Markdown/HTML exporters, so
your code never branches on the format.

```rust
// Plain text (search / indexing) — .doc or .docx, detected automatically:
let bytes = std::fs::read("report.docx")?;
let text = rdoc::extract_text(&bytes)?;
```

The `Document` API adds sub-document ranges and the rich model:

```rust
let doc = rdoc::Document::open(&bytes)?;
let body     = doc.main_text();     // main document only
let everything = doc.text();        // all sub-docs, in order

// Rich model + exporters (identical IR for .doc and .docx):
let md   = doc.to_markdown();       // # headings, **bold**, | tables |, lists, links
let html = doc.to_html();           // <h1>, <strong>, <table colspan>, <img>, <a>
let model = doc.model();            // typed IR: Vec<Block> (Paragraph | Table | Image)
let imgs  = doc.images();           // extracted PNG/JPEG bytes (like POI getAllPictures)
```

CLI (each example accepts either format):

```text
cargo run -p rdoc --example extract -- file.docx        # plain text
cargo run -p rdoc --example convert -- file.doc md      # Markdown
cargo run -p rdoc --example convert -- file.docx html   # HTML
```

### Cargo features

`.docx` support is on by default. For a dependency-light, legacy-only build (just
`cfb` + `encoding_rs` + `thiserror`, no `zip`/`quick-xml`):

```toml
rdoc = { version = "0.1", default-features = false }   # .doc only
```

## Why one crate? (and how this relates to `docx-rs`)

The mature [`docx-rs`](https://crates.io/crates/docx-rs) crate already reads (and
writes) `.docx` well, and for a `.docx`-only need it is the obvious choice. `rdoc`
adds `.docx` **not** to beat it on `.docx` features, but for **unification and
ownership**:

- **One Word dependency, one API, one IR.** A pipeline that already had to handle
  legacy `.doc` (where there is no comparable pure-Rust option) would otherwise
  carry *two* Word libraries with two different models and stitch them together.
  Here `.doc` and `.docx` produce the identical [`DocModel`] and the same
  `text()` / `to_markdown()` / `to_html()` / `images()`.
- **No JVM and no extra third-party `.docx` parser** in the dependency tree —
  only `zip` + `quick-xml` (the same pair the sibling `rxls` crate uses for
  `.xlsx`), behind a default-on feature you can switch off.

If you only ever touch `.docx`, prefer `docx-rs`. If you need both Word formats
behind one panic-free, dependency-light surface, that is what this crate is for.

## How it works

A `.docx` is a ZIP of XML parts. `rdoc` reads `word/document.xml` with
`quick-xml` by recursive descent (paragraphs `w:p` → runs `w:r`/`w:t` with
`w:rPr` bold/italic/underline; tables `w:tbl` → `w:tr`/`w:tc` with
`gridSpan`/`vMerge` → real colspan/rowspan), resolves heading levels from
`word/styles.xml` (`w:pStyle` / `Heading N` / `제목 N`), ordered-vs-bullet from
`word/numbering.xml`, hyperlink targets and image bytes from
`word/_rels/document.xml.rels` + `word/media/*`, and emits the same `Vec<Block>`
IR the `.doc` path does. Recursion is depth-capped, external XML entities are
never resolved (XXE-safe), and per-entry decompression is size-capped (zip-bomb
guard).

`.doc` is an OLE2 compound file. `rdoc`:

1. opens the container (`cfb`) and reads the `WordDocument` stream;
2. parses the **FIB** (File Information Block) by *navigating* its
   variable-length sub-structures (not hard-coded offsets) to find the piece
   table location (`fcClx`/`lcbClx`) and the sub-document char counts
   (`ccpText`/`ccpFtn`/`ccpHdd`/…);
3. reads the **CLX / piece table** from the selected `0Table`/`1Table` stream;
4. decodes each piece as UTF-16LE, or 8-bit text in the document's ANSI codepage
   when `fCompressed` is set — the codepage is derived from the FIB language id
   (`lid`), so Korean (`0x0412` → cp949), Japanese, Cyrillic, etc. decode
   correctly instead of being forced through cp1252;
5. converts Word control marks to text (paragraph/line/cell/column breaks →
   newlines, non-breaking hyphen → `-`, NBSP → space) and normalizes lines.

The **rich model** ([`Document::model`]) is a lazy second pass over the same
pieces — it never runs for the flat `text()` path. It reads the character-property
bin table (`PlcfBteChpx` → CHPX FKPs) for per-run **bold/italic/underline/strike**,
the **style sheet** (`STSH`) and outline levels for **headings**, the paragraph
properties (`PlcfBtePapx`) for **tables** (`sprmTDefTable` → real colspan/rowspan
from cell-boundary positions + merge flags) and **list autonumbers**, the field
marks for **hyperlinks**, and the `Data` stream (`PICF`) for **inline images**.
The Markdown and HTML exporters are pure folds over the resulting `Vec<Block>` IR.

Encrypted / XOR-obfuscated documents (`fEncrypted`) and pre-Word-97 (Word 6/95,
`nFib < 0x00C1`) files are detected and reported as distinct [`Error`]s rather
than silently emitting garbage. Every read is bounds-checked: malformed input
returns an [`Error`], never a panic — safe to run untrusted files in-process.

## Scope & parity

Targets POI `WordExtractor.getText()` output (flat text). Validated on the full
13-file population of real Korean government `.doc` attachments (all Word 97) at
~97.4% whitespace-insensitive parity vs POI; body-text parity is ~100%.

The **document model** reconstructs structure the flat path can't carry:
character runs (bold/italic/underline/strike/hidden) from CHPX; headings from the
STSH style sheet + outline levels (English `Heading N` and Korean `제목 N`);
merge-aware tables with real `colspan`/`rowspan` from `sprmTDefTable` cell
boundaries; list autonumbers (`1.`, `1.1`, `가.`, `(1)` … with Korean formats);
hyperlinks from field marks; and inline PNG/JPEG/GIF images extracted byte-for-byte
from the `Data` stream. Validated on the full 13-file population of real Korean
government `.doc` attachments: flat-text parity vs POI `WordExtractor.getText()` is
~97.4% (body text ~100%); every file's bold runs and tables come through, and the
embedded PNGs extract to complete, valid files.

### `.docx` parity

The `.docx` path was validated against **python-docx** as the reference oracle on
the 127-file Apache POI `document/` `.docx` test corpus (87 with comparable
extractable text). **Set-based word recall: 98.6% mean, 100% median, 85/87 files
≥ 99%.** The whole corpus runs panic-free, including adversarial fuzz cases
(`deep-table-cell.docx`, clusterfuzz inputs). The two sub-99% files are both
explained and not extraction bugs:

- one has **two `word/document.xml` entries** in the ZIP (a duplicate-name /
  zip-confusion container); `rdoc` and python-docx each pick a different copy;
- one is a heavy **tracked-changes** doc where `rdoc` shows the accept-changes
  view (insertions kept, deletions dropped) and the reference keeps the deleted
  text.

**What's still out of scope, honestly:**

- *Both formats:* metafile images (WMF/EMF — vector seals), OLE-embedded objects
  and floating Office-Art shapes (placeholders only); fields' *computed* values
  (`PAGE`, cross-references — cached result text is kept); encrypted files
  (detected and rejected, not mis-decoded).
- *`.doc` only:* per-instance list overrides (`LFOLVL` start-at); Word 6/95.
- *`.docx` only:* headers/footers, footnotes/endnotes, and comments live in
  separate parts not yet parsed, so `main_text()` equals `text()`; text inside
  drawings/shapes (`w:txbxContent`, including `mc:AlternateContent` branches) is
  not extracted; **block-level content controls (`w:sdt`) and `w:customXml` *are*
  descended into**; style-inherited emphasis is not folded (only direct `w:rPr`
  toggles are read, matching the `.doc` CHPX behavior); and exact list autonumber
  *labels* aren't reconstructed (the exporters use native list markers).

## Roadmap

- [x] Codepage-aware 8-bit pieces via FIB `lid` (cp949/EUC-KR for Korean, etc.)
- [x] Encryption/obfuscation detection and a clean Word 6/95 error gate
- [x] Full document model: character runs (CHPX), headings (STSH + outline level)
- [x] Merge-aware tables (`sprmTDefTable` → real `colspan`/`rowspan`)
- [x] List autonumbers via `PlfLst`/`PlfLfo` + `LVLF` number formats (incl. Korean)
- [x] Hyperlink fields; inline PNG/JPEG/GIF image extraction; Markdown + HTML export
- [x] **Unified `.docx` (OOXML) reader** into the same model + exporters (`zip` +
      `quick-xml`, default `docx` feature); validated at 98.6% word recall vs
      python-docx on the POI corpus
- [ ] `.docx` headers/footers, footnotes, comments, and text-box parts
- [ ] Per-instance list overrides (`LFOLVL` start-at) from `PlfLfo.rgLfoData`
- [ ] Metafile (WMF/EMF) image inflate; floating Office-Art shapes
- [ ] Field semantics (PAGE values, cross-references) matching POI
- [ ] Word 6/95 8-bit text extraction; [MS-DOC] 2.2.6.1 XOR de-obfuscation
- [ ] Fuzz tests + broad corpus regression

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The local gate is
`cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps`.

## License

Licensed under the [MIT License](LICENSE). Third-party dependency licenses are
listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md). This crate
implements only the publicly documented [MS-DOC] / [MS-CFB] specifications and
contains no Microsoft source.

[MS-DOC]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/
[MS-CFB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/
