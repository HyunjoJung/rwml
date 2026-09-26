# Generated render-oracle campaigns

This directory stores public identity locks for diagnostic campaigns whose input
documents are generated on demand. The generated DOCX files do not join the ordinary
public release corpus until their external-oracle expectations and release value have
been reviewed independently.

## Smoke and pilot inputs

`corpus/public/RENDER_SMOKE_ORACLE.json` selects 12 existing public documents
without copying them. The selection covers 35 of the parent corpus's 37 feature
labels; `alternate-content` and `top-bottom-wrap` are deliberately omitted.
It retains each input's digest, provenance, native page expectation and warnings.

`render-pilot-v1.json` locks all 21 parent documents plus 19 deterministic
synthetic OOXML fixtures. The additions exercise character paint, fields, notes,
sections, RTL lists/tables/mixed text, spacing/tabs, floating placement, revisions,
images and unequal-column continuation. Their source and provenance are
repository-owned MIT material; copied inputs retain their original provenance.
The generator closure binds the pilot, shared materializer, public fixture
generator and strict corpus contract. All 40 document and three provenance
payloads are independently size- and SHA-256-locked.

```sh
python3 -B scripts/generate_render_smoke_manifest.py --check
python3 -B scripts/generate_render_pilot.py --check
python3 -B scripts/generate_render_pilot.py \
  --output target/render-oracle/render-pilot-v1
python3 -B scripts/render_oracle_contract.py \
  target/render-oracle/render-pilot-v1/RENDER_ORACLE.json
```

Pilot output must be absent or an empty regular directory. Occupied or symlinked
destinations are rejected; payloads are staged and strictly validated before
the complete directory is published. Use a new directory for a repeat rather
than overwriting existing evidence. Both lock readers reject nonregular,
symlinked, oversized, stale or noncanonical files. Smoke output must remain
beside its parent manifest because document paths are relative to that directory;
it cannot replace the parent manifest.

The smoke and pilot manifests expect 15 and 51 native pages respectively.
These are input contracts, not Word/LibreOffice page-count assertions, completed
captures, fidelity thresholds or full-campaign acceptance. Neither selection
changes the ordinary release corpus, validation defaults or release policy.
Generator metadata changes do not relabel historical captures: retain their
original source revisions, locks and receipts even when input bytes are unchanged.

## Unequal-column tables

`unequal-table-v1.json` binds a 48-case factorial campaign across four physical column
layouts, three table-width policies, two row-fragment classes, and two continuation
handoffs. It records the generator SHA-256 plus every output path, byte length, SHA-256,
scenario label, and expected native page count. No private document content or planning
artifact is involved.

Materialize and validate the exact campaign under the ignored `target/` directory:

```sh
python3 scripts/generate_unequal_table_oracle.py
python3 scripts/generate_unequal_table_oracle.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/unequal-table-v1/RENDER_ORACLE.json
```

The generated `RENDER_ORACLE.json` uses the same bounded, path-neutral corpus contract
as `corpus/public/RENDER_ORACLE.json`. The lock is an input identity contract, not a claim of
Word parity and not a release threshold.

`scripts/table_oracle_topology.py` reduces a complete directory of `<case-id>.pdf`
outputs to content-safe structural evidence. Its producer metadata input contains only
the producer name, canonical mode, version, identity SHA-256, and platform identity.
The report retains exact input/PDF identities, page geometry, the campaign's synthetic
cell-token boxes, normalized axis-aligned table borders, and consecutive continuation
segments; it never retains arbitrary document text or local paths. Capture reports can
then be compared without defining or weakening a fidelity threshold:

```sh
python3 scripts/table_oracle_topology.py extract \
  --manifest target/render-oracle/unequal-table-v1/RENDER_ORACLE.json \
  --pdf-dir <complete-pdf-directory> \
  --producer-metadata <producer-identity.json> \
  --source-revision <full-git-sha> \
  --output <capture.json>

python3 scripts/table_oracle_topology.py compare \
  --manifest target/render-oracle/unequal-table-v1/RENDER_ORACLE.json \
  --candidate <candidate-capture.json> \
  --reference <oracle-capture.json> \
  --output <comparison.json>
```

Use `--require-normalized-exact` only when comparing two independent captures from the
same producer. Cross-producer comparisons are diagnostic until authoritative Word
evidence is reviewed.

`--source-revision` asserts the current extracting checkout's full Git SHA; it
cannot label newly extracted evidence with a different revision. The separate
producer metadata identifies the application that wrote the PDFs. Retained capture
reports keep their recorded source revisions when loaded for comparison.

## Shared diagnostic font sources

`shared-font-lock.json` extends the unchanged eight-font
`libreoffice-font-lock.json` by its exact SHA-256. It adds Noto Sans CJK KR
OpenType CFF and Noto Emoji variable TrueType sources, with immutable upstream
repository/commit/path/Git-blob identities, file sizes, SHA-256 digests, SFNT
revisions, and the two additional sources' license payloads. Its explicit
ten-font order is the intended native diagnostic order, not host font discovery.

`scripts/shared_oracle_fonts.py` prepares and verifies this source pack offline.
Obtain the eight base fonts from their existing pinned release assets and the
two additions and licenses from the commit paths in the shared lock. Retain
upstream license/provenance material; the pack does not replace the base assets'
distribution obligations. Stage exactly the ten named fonts in one directory
and the two additional license files, using their locked names, in another:

```sh
python3 -B scripts/shared_oracle_fonts.py prepare \
  --font-dir <exact-font-directory> \
  --license-dir <additional-license-directory> \
  --output target/shared-oracle-fonts
python3 -B scripts/shared_oracle_fonts.py verify \
  --output target/shared-oracle-fonts
```

Preparation requires a fresh output outside the input directories. It validates
the complete file sets before writing; verification rereads every font and
license independently and checks the exact path-neutral `MANIFEST.json`.
Missing, extra, modified, aliased, symlinked, nonregular, or oversized inputs
and inconsistent receipts are rejected. The commands do not download fonts,
install them on the host, or change an existing capture's inputs.

This establishes source identity and order only. It does not prove embedded
Type 1/CFF subset identity, variable-font instance fidelity, glyph shaping,
Word-compatible layout, or cross-producer equivalence. Existing PDF attestation
and release validation retain their separate contracts.

## Font-isolated native PDFs

The `to_pdf` example accepts repeatable `--font` arguments. Files are loaded and
registered in the supplied order, without system-font discovery or fallback:

```sh
cargo build --locked --features render --example to_pdf
target/debug/examples/to_pdf input.docx output.pdf \
  --font <regular.ttf> --font <fallback.otf> --report-json render.json
```

Use the verified shared pack's manifest order for diagnostic captures. The
example does not verify the pack or produce a campaign receipt itself. It allows
at most 128 nonempty regular font files, 64 MiB per file and 256 MiB total;
malformed options, duplicate singleton options and mixed `--font`/`--fixed-fonts`
are errors. Native paths need not be UTF-8. `--` ends option parsing for input
and output paths; use `./` for option values whose filenames begin with `-`.

Both model and `Document` fixed-font APIs return PDF bytes and metrics from the
same pass. Authored font families take precedence; supplied families then serve
as script/emoji fallbacks in input order, with multi-family files ordered by face
index. Opened documents retain source layout hints and feature warnings.
No registered font, an unembeddable registered face, or missing visible glyph
resources is a rendering error. Undecodable blobs are ignored if another font
registers; hidden text is not a rendered glyph requirement. These checks do not
prove shaping, subset source identity, cross-platform byte equality or Word layout.

The existing `--fixed-fonts` example option registers bundled Noto subsets and
still permits system fallback, as required by the current release validator.
It is not an isolated capture mode despite its historical name. Without either
font option the example keeps its system-font behavior, as do the existing
ordinary and bundled-font library APIs. Use explicit `--font` arguments for
isolation. Release defaults and acceptance gates are unchanged.

## Isolated font-program comparison

`scripts/font_subset_attestation.py` compares bounded Type 1/PFA or explicitly
mapped CID-keyed CFF subset programs against the locked Noto Sans CJK KR source.
It checks every glyph, including `.notdef`, with exact mappings, widths, font
matrices and outline operations. CID aliases, missing/extra glyphs, unsupported
font kinds or matrices, and changed outlines fail explicitly.

The parser runs only in the digest-locked Linux image described below, using
the exact pure-Python FontTools wheel in `fonttools-lock.json`. The host verifies
the wheel, stages read-only inputs, invokes the isolated worker, validates the
result, and checks that its inputs/tools/runtime did not change. No host font
installation or fallback is used. Supply the locked wheel locally:

```sh
python3 -B scripts/font_subset_attestation.py \
  --font-pack target/shared-oracle-fonts \
  --fonttools-wheel <locked-fonttools-wheel> \
  --program <subset.pfa> \
  --output target/subset-proof.json
python3 -B scripts/font_subset_attestation.py \
  --font-pack target/shared-oracle-fonts \
  --fonttools-wheel <locked-fonttools-wheel> \
  --program <subset.pfa> \
  --verify target/subset-proof.json
```

Verification recomputes the proof from the original program/source bytes in a
new worker and compares the complete receipt; it does not just trust a repaired
outer digest. Outputs must be fresh, and verification never rewrites receipts.
For raw CID-keyed CFF, also provide `--cff-glyph-map <map.json>` with schema
`rwml.cff-glyph-map.v1`, exact `source_sha256`/`subset_sha256`, and ordered
`glyphs` pairs. The map must cover `.notdef` and each canonical subset CID once,
without source aliases. This command does not discover that mapping from a PDF.

The worker checks a 2-GiB no-swap container limit and enforces a 512-MiB data
limit, 20-second CPU limit and bounded glyph/outline work. Host execution has a
30-second maximum deadline and 512-KiB stdout limit, with strict cleanup on
failure. UTF-8 JSON, numeric types, nesting and node counts are bounded before
receipt acceptance. Unsupported programs are rejected, not downgraded to
metadata-only success.

Run the explicit locked-runtime tests separately from the ordinary Python suite:

```sh
RWML_FONTTOOLS_WHEEL=<locked-fonttools-wheel> \
  python3 -B -m unittest discover -s tests/font_programs -p 'test_*.py'
```

These program comparisons do not establish PDF resource coverage, correct
ToUnicode/CMap associations, automatic native-CFF source mapping, variable-font
fidelity, shaping, or Word layout parity. The ordinary PDF metadata verifier,
campaign integration and release policy are unchanged.

## Isolated PDF font resources

`scripts/pdf_font_resources.py` connects an original PDF to its declared font
resources and exact decoded font-program/ToUnicode bytes. It walks the complete
catalog-reachable object graph, including inherited page resources, Form
XObjects, annotation appearances, AcroForm resources and ExtGState fonts.
Indirect resource references are identities; equal font names do not merge
distinct resources, and unused reachable fonts are not silently omitted.

The digest-pinned pure-Python wheel in `pypdf-lock.json` runs inside the same
locked Linux image. Supply that wheel locally; no parser or font is downloaded:

```sh
python3 -B scripts/pdf_font_resources.py \
  --pdf <original.pdf> --pypdf-wheel <locked-pypdf-wheel> \
  --output target/pdf-font-resources.json
python3 -B scripts/pdf_font_resources.py \
  --pdf <original.pdf> --pypdf-wheel <locked-pypdf-wheel> \
  --verify target/pdf-font-resources.json
```

Receipts bind PDF bytes, worker/helper code, parser wheel, runtime and limits.
Verification extracts again in a fresh worker and compares the complete receipt;
editing the resource list or repairing outer digests cannot substitute for it.
Outputs must be fresh. Receipts contain embedded font and mapping bytes, so
treat them as document-derived artifacts, not content-free diagnostics.

The bounded representations are Type 1/PFA, TrueType, Identity-H/V Type0 fonts
with one CIDFontType2 descendant, and CIDFontType0 with a CIDFontType0C stream.
Streams may be unfiltered or Flate-compressed without decoding parameters.
Missing embeddings, direct font resources, Type3 fonts, other composite
encodings, unsupported filters, external streams, encrypted PDFs and malformed
resource graphs fail explicitly. PDF name objects cannot be replaced by text
strings with the same characters. An absent ToUnicode stream is recorded as
absent, and a font-free PDF has an explicit empty inventory.

Limits are 16 MiB per PDF, 64 font resources, 16,384 graph nodes, 65,536 edges,
depth 64, 4 MiB total decoded program/mapping bytes and 64 KiB per ToUnicode
stream. The worker retains the existing 2-GiB no-swap container, 512-MiB data
and 20-second CPU limits; host execution allows at most 30 seconds and 8 MiB of
output. The parser's warnings, output overflow and timeouts are failures, with
owned-container and staging cleanup. Run explicit synthetic/hostile PDF tests:

```sh
RWML_PYPDF_WHEEL=<locked-pypdf-wheel> \
  python3 -B -m unittest discover -s tests/pdf_resources -p 'test_*.py'
```

Extraction is not font-source proof, CMap semantic validation, native glyph-map
discovery, shaping verification or Word fidelity. It is an opt-in diagnostic;
default PDF validation, renderer behavior and release policy remain unchanged.

## Native CFF mapping and proof

`scripts/native_cff_attestation.py` discovers and independently proves the
renumbered glyph mapping for every extracted native CFF resource in a PDF.
It uses the shared Noto Sans CJK KR source, the existing PDF extractor and the
mapped-program proof worker. The parser wheels and Linux image remain locked.

```sh
python3 -B scripts/native_cff_attestation.py \
  --pdf <native.pdf> --font-pack target/shared-oracle-fonts \
  --fonttools-wheel <locked-fonttools-wheel> \
  --pypdf-wheel <locked-pypdf-wheel> \
  --output target/native-cff-proof.json
python3 -B scripts/native_cff_attestation.py \
  --pdf <native.pdf> --font-pack target/shared-oracle-fonts \
  --fonttools-wheel <locked-fonttools-wheel> \
  --pypdf-wheel <locked-pypdf-wheel> \
  --verify target/native-cff-proof.json
```

ToUnicode text is a lookup hint, not evidence that the default source cmap glyph
was selected. Discovery includes bounded GSUB single/alternate substitutions,
multi-character ligatures and extension lookups. Unicode mirrored-glyph
candidates from the pinned FontTools data are included without assuming a bidi
shaping decision. Every subset glyph, including `.notdef`, must have one exact
width/outline match; no match or multiple matches is an error. The resulting
complete one-to-one map is then checked by the separate raw-CFF proof worker,
including effective font matrices. Successful discovery alone is not a proof.

The receipt binds PDF resource identities, exact CFF/ToUnicode bytes, source
font, discovery witnesses, independent proofs and all worker/tool/runtime
identities. Verification repeats extraction, discovery and proof and does not
rewrite retained evidence. It cannot accept a repaired receipt in place of
recomputation. Non-CFF resources are explicitly listed as unverified, and a
PDF with no CFF resources is not reported as a successful CFF proof.

Discovery bounds GSUB lookups/subtables/edges and ligature records, hints to
eight Unicode scalars, candidates to 256 per glyph and source signatures to
4,096. Outline and candidate-search work each have a 131,072-operation budget;
the existing per-glyph, input/output, memory and CPU bounds remain enforced.
Each isolated worker has at most 30 seconds, and the complete PDF pipeline has
a 120-second deadline. Timeouts and other failures clean up owned containers
and temporary inputs. Run the explicit locked-runtime tests with both wheels:

```sh
RWML_FONTTOOLS_WHEEL=<locked-fonttools-wheel> \
RWML_PYPDF_WHEEL=<locked-pypdf-wheel> \
  python3 -B -m unittest discover -s tests/cff_discovery -p 'test_*.py'
```

This proves the bounded source-glyph relationship for CFF resources. It does
not interpret general GSUB context, attest other font representations, prove
ToUnicode semantics or PDF text operators, or validate shaping, positioning
or Word layout. Default release gates and renderer behavior are unchanged.

## Shared-font campaign capture

`scripts/render_campaign_capture.py` composes the strict corpus manifest,
verified shared font pack, locked LibreOffice runtime and explicit native
`--font` renderer into one diagnostic capture. It requires a clean source
checkout, Rust 1.92.0 and cached Cargo dependencies; builds are offline and
locked, with a fresh target directory. Keep the Python environment and optional
NumPy installation unchanged between capture and verification.

```sh
python3 -B scripts/render_campaign_capture.py capture \
  --manifest <strict-corpus-manifest> \
  --font-pack <verified-shared-font-pack> \
  --fonttools-wheel <locked-fonttools-wheel> \
  --pypdf-wheel <locked-pypdf-wheel> \
  --output target/render-oracle/campaign
python3 -B scripts/render_campaign_capture.py verify \
  --manifest <same-strict-corpus-manifest> \
  --font-pack <same-verified-shared-font-pack> \
  --fonttools-wheel <same-locked-fonttools-wheel> \
  --pypdf-wheel <same-locked-pypdf-wheel> \
  --output target/render-oracle/campaign
```

Capture builds the native executable once and retains every source input,
native PDF/report, reference PDF/metadata and independently computed font
receipt. Verification rereads every PDF and repeats its applicable font checks.
It also builds a fresh native executable, checks its identity and replays every
case in temporary storage, requiring exact native PDF and report bytes. It
never executes the retained executable or rewrites the retained bundle.
Missing, extra or duplicated coverage, changed inputs/environment and repaired
hashes without matching recomputation fail verification.

Type 1 and native CFF resources require the exact outline proofs described
above. TrueType resources have only PostScript-name and revision checks,
explicitly labeled `postscript-and-revision-only`; this does not prove glyph
outlines, Unicode semantics or shaping. A font-free PDF records an empty
resource inventory, not a font proof.

The campaign has a four-hour elapsed-time acceptance bound and a 2-GiB retained
artifact bound. Each native render has a 120-second wall/CPU bound, a 16-MiB
per-file limit, 256 open files, 64 processes and no core dumps. The POSIX launcher
applies and reads back kernel limits before execution. Linux additionally uses
a 4-GiB address-space limit; Darwin explicitly records hard memory isolation as
unsupported. Use WSL on Windows. Font checks share a 180-second PDF budget,
with at most 30 seconds per isolated worker and 120 seconds per CFF pipeline.
Nested operations receive the remaining campaign budget; cleanup can require
additional time and failures do not produce an accepted capture receipt.

The path-neutral `CAPTURE.json` binds source, corpus, runtime, tool payloads,
ordered fonts, native build/execution settings and all artifacts. Verification
is source/runtime-bound: do not relabel historical receipts. This command does
not establish reference repeatability, Word fidelity, metric acceptance or a
release gate. Existing release validation and historical `--fixed-fonts`
fallback behavior remain unchanged.

## Capture-bound metrics and repeats

`scripts/render_campaign_metrics.py` measures retained shared-font captures using
the same stable Python environment, source checkout, corpus and locked font pack.
It first runs independent capture verification, including fresh-build native
replay, then recomputes text, integer raster and PDF geometry/semantic metrics
from retained PDFs. It never executes the retained renderer. Each document is
measured in a separate bounded process over temporary copies; original evidence
is checked again and never rewritten.

Use the manifest and runtime inputs from the capture commands above:

```sh
python3 -B scripts/render_campaign_metrics.py measure \
  --manifest corpus/public/RENDER_ORACLE.json \
  --capture target/render-oracle/capture-a \
  --font-pack target/render-oracle/shared-fonts \
  --fonttools-wheel <path-to-fonttools-4.63.0-py3-none-any.whl> \
  --pypdf-wheel <path-to-pypdf-6.16.2-py3-none-any.whl> \
  --output target/render-oracle/metrics-a.json
python3 -B scripts/render_campaign_metrics.py verify \
  --manifest corpus/public/RENDER_ORACLE.json \
  --capture target/render-oracle/capture-a \
  --font-pack target/render-oracle/shared-fonts \
  --fonttools-wheel <path-to-fonttools-4.63.0-py3-none-any.whl> \
  --pypdf-wheel <path-to-pypdf-6.16.2-py3-none-any.whl> \
  --evidence target/render-oracle/metrics-a.json
```

The v8 report binds the exact v3 capture receipt, source/environment, renderer,
manifest, ordered cases, PDF/report/font-receipt hashes and complete reference
page raster hashes, including dimensions and color mode. It requires the locked
shared-font profile, complete page coverage and zero skipped cases. Page counts
beyond the configured cap are rejected, not silently measured as partial
coverage. `--dpi`, `--page-cap` and `--recall-min` must match for verification;
their defaults are the existing raster settings and 0.97 recall. Single-capture
reports do not claim reference repeatability.

After independently capturing and measuring a second complete run at the same
source and environment, verify both reports and the retained pair:

```sh
python3 -B scripts/render_campaign_metrics.py repeat \
  --manifest corpus/public/RENDER_ORACLE.json \
  --first-capture target/render-oracle/capture-a \
  --second-capture target/render-oracle/capture-b \
  --first-evidence target/render-oracle/metrics-a.json \
  --second-evidence target/render-oracle/metrics-b.json \
  --font-pack target/render-oracle/shared-fonts \
  --fonttools-wheel <path-to-fonttools-4.63.0-py3-none-any.whl> \
  --pypdf-wheel <path-to-pypdf-6.16.2-py3-none-any.whl> \
  --output target/render-oracle/repeat.json
```

The v2 repeat verifier requires distinct roots and report paths, independently
recomputes both reports, and compares exact native PDF/report/font-receipt bytes,
all reference page rasters and metric content outside capture-specific bindings.
Matching fabricated reports cannot pass solely because their hashes or aggregates
agree. Both artifact sets, environment and source are rechecked after measurement.
Reference PDF metadata may differ; the complete reference rasters must match.
Verification streams cases rather than retaining all campaign PDFs in memory.

Each metric child has a 180-second wall/CPU limit, 16 MiB file-size limit,
8 MiB output limit, 256 descriptors, 64 processes and no core dumps. Linux also
enforces a 4 GiB address-space limit; Darwin does not claim hard memory isolation.
Use WSL on Windows. The existing four-hour capture-verification limit is
unchanged; metric measurement has a separate four-hour budget. Repeat checks
the combined sixteen-hour acceptance budget for both verification/measurement
pairs, including final checks; this is a ceiling, not an expected duration.

Successful measurement or repeat verification means the diagnostic evidence is
valid, not that fidelity passes. Reports retain the recall/skip gate and repeat
receipts expose `fidelity_gate_passed`, including `false`. There is no Word parity
or release-acceptance claim. Type 1/CFF proof and metadata-only TrueType coverage
remain distinct. Existing v4 evidence, legacy release defaults and thresholds
are unchanged; historical v5-v7 reports are not silently accepted as v8.

## Locked LibreOffice runtime

`scripts/libreoffice_container.py` prepares and verifies a fixed Linux amd64
LibreOffice Writer image. `libreoffice-container-lock.json` pins the upstream
archive, base image, BuildKit/Buildx versions, recipe, profile, font configuration,
image manifest, config, and uncompressed layer digests. The runtime requires a
POSIX Docker client and a Linux daemon capable of executing amd64 images; use
WSL for this path on Windows. It does not replace the local release-preflight
oracle or change release acceptance.

With Buildx 0.35.0 installed, prepare a fresh build context from the official
archive. The preparation command verifies the archive bytes before creating the
context and refuses an existing output directory:

```sh
mkdir -p target/libreoffice-oracle
curl --fail --location --proto '=https' --proto-redir '=https' \
  --output target/libreoffice-oracle/libreoffice.tar.gz \
  https://downloadarchive.documentfoundation.org/libreoffice/old/26.2.3.2/deb/x86_64/LibreOffice_26.2.3.2_Linux_x86-64_deb.tar.gz
python3 scripts/libreoffice_container.py prepare \
  --archive target/libreoffice-oracle/libreoffice.tar.gz \
  --output target/libreoffice-oracle/context
docker buildx create --name rwml-lo-build --driver docker-container \
  --driver-opt image=docker.io/moby/buildkit:v0.31.2@sha256:2f5adac4ecd194d9f8c10b7b5d7bceb5186853db1b26e5abd3a657af0b7e26ec \
  --buildkitd-flags '--oci-worker-snapshotter=native' --bootstrap
docker buildx build --builder rwml-lo-build --platform linux/amd64 \
  --file target/libreoffice-oracle/context/Containerfile \
  --build-arg SOURCE_DATE_EPOCH=1783900800 \
  --no-cache --provenance=false --sbom=false \
  --output type=docker,dest=target/libreoffice-oracle/image.tar,rewrite-timestamp=true,oci-mediatypes=false \
  target/libreoffice-oracle/context
docker load --input target/libreoffice-oracle/image.tar
python3 scripts/libreoffice_container.py inspect
```

Use a fresh builder name if the example name is occupied. Inspection accepts
both Docker image-store formats only after checking the locked manifest/config
identity, complete layer identity, platform, user, entrypoint, and working
directory. Capture uses the validated digest with `--pull never`; it never
substitutes a floating tag. The recipe removes installed fonts and a
host-dependent fontconfig installation log. Do not update the lock to accept a
different build output without investigating the difference.

The Python `capture_document` helper runs one staged DOCX with a separately
staged font directory. The source directory contains `input.docx` and
`SHA256SUMS`; the font directory contains the selected font files, `SHA256SUMS`,
and `expected-paths.txt` listing their exact `/oracle/fonts/` paths. The caller
must bind these staged bytes to its corpus and font locks. The container checks
both checksum sets before and after conversion and checks its complete visible
font list. This low-level runtime does not attest a campaign or select fonts.

Each call uses a fresh profile in a read-only, non-root container with no network
or capabilities, two CPUs, 2 GiB memory with no extra swap, 128 PIDs, bounded
temporary storage, and a 180-second attached-process deadline. Output is a
bounded archive with exactly six regular-file members: the PDF, version, font
list, PDF checksum, warmup log, and conversion log. Duplicate, missing, linked,
sparse, or oversized members are rejected. The task-owned container is removed
on success and failure, and cleanup failure is reported. Process deadlines must
be finite positive numbers and output limits must be bounded positive integers.

Runtime inspection and a successful conversion alone do not prove repeatability,
Word parity, or full-campaign acceptance.

## Repeated LibreOffice table capture

`scripts/libreoffice_table_capture.py` runs both complete exports of the 48-case
unequal-column table corpus and independently validates the retained results.
It uses the locked runtime above and the exact `NotoSans-Regular.ttf` bytes from
`word-font-lock.json`. PyMuPDF 1.28.2 and Pillow 12.3.0 are required. The font is
staged read-only; this command does not download it or install host fonts.

From a clean checkout and a stable Python environment, use a fresh output path:

```sh
python3 -B scripts/libreoffice_table_capture.py capture \
  --font <path-to-NotoSans-Regular.ttf> \
  --output target/libreoffice-oracle/table-capture
python3 -B scripts/libreoffice_table_capture.py validate \
  --output target/libreoffice-oracle/table-capture
```

Each capture records the checkout, Python executable and runtime payload,
imported library payloads, Docker executable/runtime, image/recipe, input, font,
and extraction-harness identities. The analysis identity includes installed
startup code and bytecode, not only version strings or installer receipts.
Keep the environment unchanged and use `-B` for capture and validation. A
symlinked or changing library payload is rejected rather than silently omitted.

The verifier checks both complete PDF/metadata sets, exact artifact checksums,
embedded font PostScript names and SFNT revisions, independently extracted table
topology, and every page's 110-DPI raster. Raster hashes bind width, height and
color mode; a truncated page set is not a complete stability observation.
Only metadata reads explicitly allow empty regular files, such as warmup logs;
empty inputs, links, FIFOs and changing files remain invalid. Font names and
revision metadata alone do not prove outline or Unicode correctness.

`CAPTURE.json` is written only after independent validation succeeds. All 48
normalized topologies and complete raster sets must repeat. PDF bytes are
retained but are not required to match, because producer metadata may differ.
Validation uses the retained font and regenerated input identities and does not
launch Docker or Word. It requires the recorded checkout, harness and analysis
environment; historical receipts must not be relabeled to another revision.

These are diagnostic captures, not an authoritative Word comparison or a
release gate. General font packs and the expanded native-versus-oracle campaigns
remain separate from this single-font table path. `libreoffice-font-lock.json`
also records the bounded OFL font provenance used by the shared font-inspection
helpers; it does not widen the table capture's accepted font set.

## Microsoft Word diagnostic capture

`word-font-lock.json` identifies the exact Noto Sans Regular font used by this
campaign. It pins the official
[`NotoSans-v2.015`](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015)
release tag, target commit, release-archive byte length and SHA-256, archive member,
and extracted font identity. The font is licensed under SIL Open Font License 1.1,
but the font binary is not copied into this repository. A capture host must install
the exact locked file in the Windows system or per-user font directory; the harness
verifies its byte length and SHA-256 before opening Word and verifies the embedded PDF
PostScript font name after export.

The authoritative diagnostic backend requires desktop Microsoft Word on Windows,
Windows PowerShell, PyMuPDF, and a clean checkout at the full source revision being
captured. It disables macros and dialogs, opens every generated DOCX read-only, uses a
fixed `ExportAsFixedFormat` option set, records the Word executable and runtime
identity, and retains no local paths. It does not use the network.

Run the campaign from PowerShell with the exact installed font path:

```powershell
python scripts/word_oracle_capture.py capture `
  --font "$env:WINDIR\Fonts\NotoSans-Regular.ttf" `
  --source-revision (git rev-parse HEAD)
```

The command creates two fresh Word processes beneath the ignored `target/` tree,
validates all 48 PDF identities and embedded fonts, extracts both topology reports,
and requires all 48 normalized reports to match exactly. Transient jobs containing
local paths are deleted after successful runs. `CAPTURE.json`, each path-neutral
export metadata file, both topology captures, and `repeatability.json` retain the
evidence needed for review.

Microsoft Word evidence remains diagnostic until the captured topology has been
reviewed against the renderer and accepted publicly. A repeatable capture alone does
not define a parity threshold, change renderer behavior, or add a release gate.
