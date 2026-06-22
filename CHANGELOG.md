# Changelog

All notable changes to `rdoc` are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security
- **Tighter zip-bomb bound for `.docx`** (`docx/mod.rs`): each ZIP part's
  decompressed size is capped at 64 MiB (was 256 MiB) and rejected up front when
  the ZIP-declared uncompressed size exceeds it, so a ~200 KB `.docx` that inflates
  to gigabytes is refused in milliseconds with a clean error instead of burning
  ~1 GiB / ~16 s.

### Fixed
- **Field instruction state machine** (`assemble.rs`): a field with no `0x14`
  separator (`0x13 … 0x15`) left the model assembler stuck in "instruction" mode,
  silently dropping *all* document content after it. Instruction tracking is now
  a per-field stack (visible only when every open field has passed its
  separator), which also correctly hides a nested field's result when it sits
  inside an enclosing field's instruction. Recovered substantial structure across
  the 426-file corpus (tables 17.1k → 20.5k, headings 4.5k → 4.9k, bold runs
  14.1k → 16.2k, images 618 → 657) with no new panics.
- **Inline image magic scan** (`image.rs`): the raster-signature scan returned
  the first byte offset where *any* magic matched, so a chance `FF D8 FF` triple
  in the Escher/OLE binary pre-empted a real later PNG, and the 3-byte JPEG magic
  had no marker validation — extracting tens of KB of garbage as `image/jpeg`
  (observed on GovDocs1 `000_000130.doc` as `FF D8 FF 85`, where `0x85` is not a
  JPEG marker). Now each format is scanned over the whole payload in reliability
  order (PNG → GIF → JPEG), and JPEG requires `FF D8 FF <marker ≥ 0xC0>`. Suspect
  extractions on the corpus dropped from 2 to 0.
- **CHPX FC alignment for undecodable bytes** (`assemble.rs::decode_with_fc`): an
  8-bit byte with no mapping in the document codepage decodes to U+FFFD, and the
  per-character re-encode used to size its source bytes turned that into a
  multi-byte numeric character reference (`&#65533;`), over-counting and shifting
  every following character's FC so its CHPX (bold/italic/…) was misattributed.
  The byte width is now clamped to the real 1–2 byte range with a round-trip
  error treated as a single source byte.

### Added
- **Unified `.docx` (OOXML WordprocessingML) reader.** `rdoc` now reads modern
  `.docx` in addition to legacy `.doc`, into the **same** [`DocModel`] and the
  same `text` / `to_markdown` / `to_html` / `images` surfaces.
  [`Document::open`] format-detects from the magic bytes (OLE2 `D0CF11E0` → `.doc`,
  ZIP `PK` → `.docx`). The `.docx` path (behind a default-on `docx` cargo feature
  using `zip` + `quick-xml`, mirroring the sibling `rxls` `.xlsx` setup) parses
  `word/document.xml` (paragraphs/runs with bold/italic/underline; tables with
  `gridSpan`/`vMerge` → colspan/rowspan), `word/styles.xml` (heading levels:
  `Heading N` / `제목 N`), `word/numbering.xml` (ordered vs bullet),
  `word/_rels` + `word/media` (hyperlink targets and inline images). It is
  recursion-depth-capped (no stack-overflow on pathological nesting), XXE-safe
  (external XML entities are never resolved), and zip-bomb-guarded. Disable with
  `default-features = false` for a dependency-light `.doc`-only build. The goal is
  *unification + ownership* (one Word crate, one IR, no JVM, no external `.docx`
  dependency) — not to outdo the mature `docx-rs` on `.docx` features; see the
  README. Validated against python-docx on the 127-file Apache POI `.docx` corpus:
  **98.6% mean / 100% median set-word recall, 85/87 files ≥ 99%, 0 panics.**
- **Full document model + Markdown/HTML export.** Beyond flat text, `rdoc` now
  builds a lazy typed IR (`Document::model` → `Vec<Block>`) and renders
  `Document::to_markdown` / `Document::to_html` — no other Rust crate does this
  for the legacy binary `.doc` format. Components:
  - **Character runs** (`chpx.rs`): per-run bold/italic/underline/strike/hidden
    from the CHPX bin table (`PlcfBteChpx` → CHPX FKPs).
  - **Headings** (`stsh.rs`): from the STSH style sheet (`sti` 1–9, `istdBase`
    chain) and outline level (`sprmPOutLvl`), matching English `Heading N` and
    Korean `제목 N`.
  - **Merge-aware tables** (`table.rs`): `sprmTDefTable` → real `colspan`
    (global cell-boundary set + `fMerged`) and `rowspan`
    (`fVertRestart`/`fVertMerge` by column), with a GFM/HTML-fallback exporter.
  - **Hyperlinks** from field marks (`0x13`/`0x14`/`0x15`), and **inline
    PNG/JPEG/GIF images** extracted byte-for-byte from the `Data` stream
    (`image.rs`; `Document::images()` ≈ POI `getAllPictures`).
  - Validated on the 13-file Korean government corpus; the fast `text()` path is
    untouched (still ~97.4% POI parity).
- Initial public release of `rdoc`.
- `extract_text(&[u8]) -> Result<String>` convenience entry point.
- `Document` API: `open`, `text`, `main_text`, `footnote_text`, `header_text`,
  `char_count`, `is_complex`.
- OLE2/CFB container access, FIB parsing via variable-length navigation
  (csw/cslw), CLX/piece-table decoding, UTF-16LE and cp1252 (`fCompressed`)
  piece decoding, Word control-mark handling, and line normalization.
- Typed [`Error`] enum; panic-free, bounds-checked parsing.
- Synthetic-`.doc` round-trip tests, `extract` example, README, and MIT license.

### Changed
- Compressed (8-bit) pieces are now decoded in the document's ANSI codepage
  derived from the FIB language id (`lid`) — Korean `0x0412` → cp949/EUC-KR,
  Japanese → cp932, etc. — instead of being hard-coded to cp1252. (Informed by
  LibreOffice ww8 `GetCharSetFromLanguage`, antiword, catdoc.)
- Control-mark handling: column break (`0x0E`) → newline, non-breaking hyphen
  (`0x1E`) → `-`, non-breaking space (`0xA0`) → regular space, and the `0x09`
  tab is now **preserved** (it is real content that POI keeps) instead of being
  dropped with other control characters.

### Added
- **Table reconstruction** (first step toward a full parser): parse the
  `PlcfBtePapx` bin table and PAPX FKP pages ([MS-DOC] 2.8.25 / 2.9.137) to
  recover each paragraph's `fInTable`/`fTtp` flags, and render table cell marks
  (`0x07`) as POI does — cells tab-separated, rows newline-separated. Degrades
  gracefully (newline per cell) when paragraph properties are unavailable.
- **List autonumber reconstruction**: parse `PlfLst`/`LSTF`/`LVL`/`LVLF` and
  `PlfLfo`/`LFO` ([MS-DOC] 2.9.150/.131/.132/.133/.149/.129), read each
  paragraph's `ilfo`/`ilvl`, and compute the rendered label (`1.`, `1.1`, `가.`,
  `(1)` …) with per-level counters, restart-on-higher-level, and the level's
  `xst` number template + `ixchFollow` separator. Number formats cover decimal,
  roman, letter, ordinal, circled, decimal-zero, and **Korean** (Ganada `가나다`,
  Chosung `ㄱㄴㄷ`, Sino-Korean `일이삼`, native counting). Generated labels go
  into `text()` only; `raw`/the sub-document accessors stay aligned with Word's
  CP space.

### Security / robustness
- Detect encrypted / XOR-obfuscated documents (`fEncrypted`/`fObfuscated`) and
  return `Error::Encrypted` instead of indexing scrambled bytes.
- Detect pre-Word-97 (Word 6/95, `nFib < 0x00C1`) and return
  `Error::UnsupportedVersion` so callers can route to a fallback extractor.

[Unreleased]: https://github.com/HyunjoJung/rdoc/commits/main
